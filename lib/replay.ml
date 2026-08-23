type kind = Time | Random | Network
type request = { kind : kind; key : string; request_digest : string }
type response = { status : int; body : string }
type tape = { version : int; exchanges : (request * response) list }

let kind_to_string = function
  | Time -> "time"
  | Random -> "random"
  | Network -> "network"

let kind_of_string = function
  | "time" -> Ok Time
  | "random" -> Ok Random
  | "network" -> Ok Network
  | value -> Error [ "unknown replay kind: " ^ value ]

let exchange_to_yojson (request, response) =
  `Assoc
    [
      ("kind", `String (kind_to_string request.kind));
      ("key", `String request.key);
      ("request_digest", `String request.request_digest);
      ("status", `Int response.status);
      ("body", `String response.body);
    ]

let to_yojson tape =
  `Assoc
    [
      ("version", `Int tape.version);
      ("exchanges", `List (List.map exchange_to_yojson tape.exchanges));
    ]

let encode_string tape = Yojson.Safe.to_string (to_yojson tape) ^ "\n"

let decode_exchange index = function
  | `Assoc fields ->
      let path name = Printf.sprintf "exchanges[%d].%s" index name in
      let string name =
        match List.assoc_opt name fields with
        | Some (`String value) -> Ok value
        | Some _ -> Error [ path name ^ " must be a string" ]
        | None -> Error [ path name ^ " is required" ]
      in
      let int name =
        match List.assoc_opt name fields with
        | Some (`Int value) -> Ok value
        | Some _ -> Error [ path name ^ " must be an integer" ]
        | None -> Error [ path name ^ " is required" ]
      in
      begin match string "kind" with
      | Error _ as error -> error
      | Ok raw_kind ->
          begin match kind_of_string raw_kind with
          | Error _ as error -> error
          | Ok kind ->
              begin match string "key" with
              | Error _ as error -> error
              | Ok key ->
                  begin match string "request_digest" with
                  | Error _ as error -> error
                  | Ok request_digest ->
                      begin match int "status" with
                      | Error _ as error -> error
                      | Ok status ->
                          begin match string "body" with
                          | Error _ as error -> error
                          | Ok body ->
                              Ok
                                ({ kind; key; request_digest }, { status; body })
                          end
                      end
                  end
              end
          end
      end
  | _ -> Error [ Printf.sprintf "exchanges[%d] must be an object" index ]

let decode_exchanges = function
  | `List values ->
      let rec loop index accumulator = function
        | [] -> Ok (List.rev accumulator)
        | value :: rest ->
            begin match decode_exchange index value with
            | Error _ as error -> error
            | Ok exchange -> loop (index + 1) (exchange :: accumulator) rest
            end
      in
      loop 0 [] values
  | _ -> Error [ "exchanges must be an array" ]

let of_yojson = function
  | `Assoc fields ->
      begin match List.assoc_opt "version" fields with
      | Some (`Int 1) ->
          begin match List.assoc_opt "exchanges" fields with
          | Some value ->
              Result.map
                (fun exchanges -> { version = 1; exchanges })
                (decode_exchanges value)
          | None -> Error [ "replay tape is missing exchanges" ]
          end
      | Some (`Int version) ->
          Error [ Printf.sprintf "unsupported replay version: %d" version ]
      | Some _ -> Error [ "replay version must be an integer" ]
      | None -> Error [ "replay tape is missing version" ]
      end
  | _ -> Error [ "replay tape must be an object" ]

let decode_string source =
  try Yojson.Safe.from_string source |> of_yojson
  with Yojson.Json_error message ->
    Error [ "invalid replay JSON: " ^ message ]

module Recorder = struct
  type t = { mutable reversed : (request * response) list }

  let create () = { reversed = [] }

  let record ?(secret = false) recorder request response =
    let request, response =
      if secret then
        ( { request with key = "redacted:" ^ Sha256.hex request.key },
          { response with body = "redacted:" ^ Sha256.hex response.body } )
      else (request, response)
    in
    recorder.reversed <- (request, response) :: recorder.reversed

  let finish recorder = { version = 1; exchanges = List.rev recorder.reversed }
end

module Player = struct
  type t = { mutable remaining : (request * response) list }

  let create tape = { remaining = tape.exchanges }

  let describe request =
    Printf.sprintf "%s %s %s"
      (kind_to_string request.kind)
      request.key request.request_digest

  let next ?(secret = false) player request =
    let request =
      if secret then { request with key = "redacted:" ^ Sha256.hex request.key }
      else request
    in
    match player.remaining with
    | [] -> Error ("replay exhausted; unexpected request: " ^ describe request)
    | (expected, response) :: rest ->
        if expected = request then begin
          player.remaining <- rest;
          Ok response
        end
        else
          Error
            (Printf.sprintf "replay mismatch: expected %s, received %s"
               (describe expected) (describe request))

  let finish player =
    match List.length player.remaining with
    | 0 -> Ok ()
    | count ->
        Error
          (Printf.sprintf "%d unconsumed replay exchange%s" count
             (if count = 1 then "" else "s"))
end

let wrap_backend player (backend : Runner.backend) =
  {
    backend with
    network_request =
      (fun ~method_ ~uri ->
        let request =
          {
            kind = Network;
            key = method_ ^ " " ^ uri;
            request_digest = Sha256.hex "";
          }
        in
        match Player.next player request with
        | Error _ as error -> error
        | Ok response when response.status >= 200 && response.status < 400 ->
            Ok response.body
        | Ok response ->
            Error
              (Printf.sprintf "replayed network response has status %d"
                 response.status));
  }
