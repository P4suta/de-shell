type transport = {
  exchange : string -> (string, string) result;
  close : unit -> unit;
}

type server = { name : string; version : string; capabilities : string list }

type t = {
  transport : transport;
  max_bytes : int;
  mutable next_id : int;
  mutable server : server option;
  mutable closed : bool;
}

let create ~max_bytes transport =
  if max_bytes <= 0 then invalid_arg "adapter max_bytes must be positive";
  { transport; max_bytes; next_id = 1; server = None; closed = false }

let close client =
  if not client.closed then begin
    client.closed <- true;
    client.transport.close ()
  end

let close_noerr descriptor =
  try Unix.close descriptor with Unix.Unix_error _ -> ()

let process_status pid =
  try
    match Unix.waitpid [ Unix.WNOHANG ] pid with
    | 0, _ -> ""
    | _, Unix.WEXITED code -> Printf.sprintf " (exit %d)" code
    | _, Unix.WSIGNALED signal -> Printf.sprintf " (signal %d)" signal
    | _, Unix.WSTOPPED signal -> Printf.sprintf " (stopped %d)" signal
  with Unix.Unix_error _ -> ""

let connect_process ~program ~arguments ~timeout_seconds ~max_bytes () =
  if program = "" then Error "adapter program must not be empty"
  else if timeout_seconds <= 0.0 then
    Error "adapter timeout_seconds must be positive"
  else if max_bytes <= 0 then Error "adapter max_bytes must be positive"
  else
    let child_stdin, parent_stdin = Unix.pipe () in
    let parent_stdout, child_stdout = Unix.pipe () in
    let close_all () =
      close_noerr child_stdin;
      close_noerr parent_stdin;
      close_noerr parent_stdout;
      close_noerr child_stdout
    in
    try
      let argv = Array.of_list (program :: arguments) in
      let pid =
        Unix.create_process program argv child_stdin child_stdout Unix.stderr
      in
      close_noerr child_stdin;
      close_noerr child_stdout;
      let alive = Atomic.make true in
      let exchange_lock = Mutex.create () in
      let pending = ref "" in
      let terminate () =
        if Atomic.compare_and_set alive true false then begin
          (try Unix.kill pid Sys.sigkill with Unix.Unix_error _ -> ());
          close_noerr parent_stdin;
          close_noerr parent_stdout;
          ignore (process_status pid)
        end
      in
      let terminate_before_join () =
        if Atomic.compare_and_set alive true false then begin
          (try Unix.kill pid Sys.sigkill with Unix.Unix_error _ -> ());
          close_noerr parent_stdin;
          (* Reap the child before joining: its closed stdout releases the
             blocked reader on Windows without closing that descriptor from a
             competing thread. *)
          try ignore (Unix.waitpid [] pid) with Unix.Unix_error _ -> ()
        end
      in
      let disconnected () =
        "adapter process disconnected" ^ process_status pid
      in
      let write_request raw =
        let payload = raw ^ "\n" in
        let rec loop offset =
          if offset = String.length payload then Ok ()
          else
            try
              let written =
                Unix.write_substring parent_stdin payload offset
                  (String.length payload - offset)
              in
              if written = 0 then Error (disconnected ())
              else loop (offset + written)
            with
            | Unix.Unix_error ((Unix.EPIPE | Unix.EBADF), _, _) ->
                Error (disconnected ())
            | Unix.Unix_error (error, operation, _) ->
                Error
                  (Printf.sprintf "adapter process %s failed: %s" operation
                     (Unix.error_message error))
        in
        loop 0
      in
      let trim_carriage_return value =
        let length = String.length value in
        if length > 0 && value.[length - 1] = '\r' then
          String.sub value 0 (length - 1)
        else value
      in
      let read_response () =
        let buffer = Buffer.create 4096 in
        let rec consume text =
          match String.index_opt text '\n' with
          | Some index ->
              Buffer.add_substring buffer text 0 index;
              pending :=
                String.sub text (index + 1) (String.length text - index - 1);
              let value = Buffer.contents buffer |> trim_carriage_return in
              if String.length value > max_bytes then
                Error
                  (Printf.sprintf "adapter response exceeds the %d byte limit"
                     max_bytes)
              else Ok value
          | None ->
              Buffer.add_string buffer text;
              if Buffer.length buffer > max_bytes then
                Error
                  (Printf.sprintf "adapter response exceeds the %d byte limit"
                     max_bytes)
              else read_more ()
        and read_more () =
          let bytes = Bytes.create 4096 in
          try
            match Unix.read parent_stdout bytes 0 (Bytes.length bytes) with
            | 0 -> Error (disconnected ())
            | count -> consume (Bytes.sub_string bytes 0 count)
          with
          | Unix.Unix_error ((Unix.EPIPE | Unix.EBADF), _, _) ->
              Error (disconnected ())
          | Unix.Unix_error (error, operation, _) ->
              Error
                (Printf.sprintf "adapter process %s failed: %s" operation
                   (Unix.error_message error))
        in
        let initial = !pending in
        pending := "";
        if initial = "" then read_more () else consume initial
      in
      let exchange raw =
        if not (Atomic.get alive) then Error "adapter process is closed"
        else begin
          Mutex.lock exchange_lock;
          let outcome = ref None in
          let outcome_lock = Mutex.create () in
          let worker =
            Thread.create
              (fun () ->
                let result =
                  match write_request raw with
                  | Error _ as error -> error
                  | Ok () -> read_response ()
                in
                Mutex.lock outcome_lock;
                outcome := Some result;
                Mutex.unlock outcome_lock)
              ()
          in
          let deadline = Unix.gettimeofday () +. timeout_seconds in
          let rec await () =
            Mutex.lock outcome_lock;
            let result = !outcome in
            Mutex.unlock outcome_lock;
            match result with
            | Some value -> Some value
            | None when Unix.gettimeofday () >= deadline -> None
            | None ->
                Thread.delay 0.005;
                await ()
          in
          let result =
            match await () with
            | Some result ->
                Thread.join worker;
                result
            | None ->
                terminate_before_join ();
                Thread.join worker;
                close_noerr parent_stdout;
                Error
                  (Printf.sprintf "adapter process timed out after %.3g seconds"
                     timeout_seconds)
          in
          Mutex.unlock exchange_lock;
          result
        end
      in
      Ok (create ~max_bytes { exchange; close = terminate })
    with
    | Unix.Unix_error (error, operation, path) ->
        close_all ();
        Error
          (Printf.sprintf "could not start adapter (%s %s): %s" operation path
             (Unix.error_message error))
    | exception_ ->
        close_all ();
        Error ("could not start adapter: " ^ Printexc.to_string exception_)

let request client ~method_ ~params =
  if client.closed then Error "adapter client is closed"
  else
    let id = client.next_id in
    client.next_id <- client.next_id + 1;
    let json =
      `Assoc
        [
          ("jsonrpc", `String "2.0");
          ("id", `Int id);
          ("method", `String method_);
          ("params", params);
        ]
      |> Yojson.Safe.to_string
    in
    if String.length json > client.max_bytes then
      Error
        (Printf.sprintf "adapter request exceeds the %d byte limit"
           client.max_bytes)
    else
      match client.transport.exchange json with
      | Error _ as error -> error
      | Ok raw_response ->
          if String.length raw_response > client.max_bytes then
            Error
              (Printf.sprintf "adapter response exceeds the %d byte limit"
                 client.max_bytes)
          else
            begin try
              match Yojson.Safe.from_string raw_response with
              | `Assoc fields ->
                  begin match List.assoc_opt "jsonrpc" fields with
                  | Some (`String "2.0") ->
                      begin match List.assoc_opt "id" fields with
                      | Some (`Int response_id) when response_id = id ->
                          let result = List.assoc_opt "result" fields in
                          let error = List.assoc_opt "error" fields in
                          begin match (result, error) with
                          | Some value, None -> Ok value
                          | None, Some (`Assoc error_fields) ->
                              let code =
                                match List.assoc_opt "code" error_fields with
                                | Some (`Int value) -> string_of_int value
                                | _ -> "unknown"
                              in
                              let message =
                                match List.assoc_opt "message" error_fields with
                                | Some (`String value) -> value
                                | _ -> "malformed adapter error"
                              in
                              Error
                                (Printf.sprintf "adapter RPC error %s: %s" code
                                   message)
                          | None, Some _ ->
                              Error "adapter error must be a JSON object"
                          | Some _, Some _ ->
                              Error
                                "adapter response cannot contain both result \
                                 and error"
                          | None, None ->
                              Error
                                "adapter response must contain result or error"
                          end
                      | Some _ ->
                          Error "adapter response id does not match request"
                      | None -> Error "adapter response is missing id"
                      end
                  | _ -> Error "adapter response jsonrpc must be 2.0"
                  end
              | _ -> Error "adapter response must be a JSON object"
            with Yojson.Json_error message ->
              Error ("invalid adapter response JSON: " ^ message)
            end

let decode_server = function
  | `Assoc fields ->
      begin match List.assoc_opt "protocol_version" fields with
      | Some (`Int version) when version = Plugin_protocol.protocol_version ->
          begin match List.assoc_opt "server" fields with
          | Some (`Assoc server_fields) ->
              begin match
                ( List.assoc_opt "name" server_fields,
                  List.assoc_opt "version" server_fields,
                  List.assoc_opt "capabilities" fields )
              with
              | ( Some (`String name),
                  Some (`String version),
                  Some (`List raw_capabilities) ) ->
                  let rec capabilities accumulator = function
                    | [] ->
                        Ok
                          { name; version; capabilities = List.rev accumulator }
                    | `String value :: rest ->
                        capabilities (value :: accumulator) rest
                    | _ -> Error "adapter capabilities must be strings"
                  in
                  capabilities [] raw_capabilities
              | _ ->
                  Error
                    "adapter handshake requires server name/version and \
                     capabilities"
              end
          | _ -> Error "adapter handshake server must be an object"
          end
      | Some (`Int version) ->
          Error
            (Printf.sprintf
               "adapter protocol version %d is incompatible with version %d"
               version Plugin_protocol.protocol_version)
      | _ -> Error "adapter handshake protocol_version must be an integer"
      end
  | _ -> Error "adapter handshake result must be an object"

let handshake client =
  match client.server with
  | Some server -> Ok server
  | None ->
      begin match
        request client ~method_:"deshell.handshake"
          ~params:
            (`Assoc
               [
                 ("protocol_version", `Int Plugin_protocol.protocol_version);
                 ( "client",
                   `Assoc
                     [
                       ("name", `String "deshell"); ("version", `String "0.1.0");
                     ] );
               ])
      with
      | Error _ as error -> error
      | Ok result ->
          begin match decode_server result with
          | Error _ as error -> error
          | Ok server ->
              client.server <- Some server;
              Ok server
          end
      end

let call client ~method_ ~params =
  match client.server with
  | None -> Error "adapter handshake is required before method calls"
  | Some server ->
      if not (List.mem method_ server.capabilities) then
        Error ("adapter did not advertise capability " ^ method_)
      else request client ~method_ ~params
