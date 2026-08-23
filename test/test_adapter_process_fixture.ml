open Deshell

let response ~id result =
  `Assoc [ ("jsonrpc", `String "2.0"); ("id", id); ("result", result) ]

let handle request =
  let open Yojson.Safe.Util in
  let method_ = request |> member "method" |> to_string in
  let id = Plugin_protocol.response_id request in
  match method_ with
  | "deshell.handshake" ->
      Plugin_protocol.handle_handshake ~server_name:"process-fixture"
        ~capabilities:[ "frontend.detect"; "frontend.hang"; "frontend.large" ]
        request
  | "frontend.detect" ->
      response ~id
        (`Assoc
           [
             ("interpreter", `String "powershell");
             ("parser", `String "official-ast");
           ])
  | "frontend.hang" ->
      Unix.sleepf 5.0;
      response ~id `Null
  | "frontend.large" ->
      response ~id (`Assoc [ ("blob", `String (String.make 8192 'x')) ])
  | _ ->
      Plugin_protocol.error_response ~id ~code:(-32601)
        ~message:"method not found"

let () =
  if Array.length Sys.argv > 1 && Sys.argv.(1) = "--exit-immediately" then
    exit 23;
  try
    while true do
      let request = input_line stdin |> Yojson.Safe.from_string in
      handle request |> Yojson.Safe.to_string |> print_endline;
      flush stdout
    done
  with End_of_file -> ()
