let protocol_version = 1
let version = protocol_version

let response_id = function
  | `Assoc fields -> Option.value ~default:`Null (List.assoc_opt "id" fields)
  | _ -> `Null

let error_response ~id ~code ~message =
  `Assoc
    [
      ("jsonrpc", `String "2.0");
      ("id", id);
      ("error", `Assoc [ ("code", `Int code); ("message", `String message) ]);
    ]

let handle_handshake ~server_name ~capabilities request =
  let id = response_id request in
  match request with
  | `Assoc fields ->
      begin match
        (List.assoc_opt "jsonrpc" fields, List.assoc_opt "method" fields)
      with
      | Some (`String "2.0"), Some (`String "deshell.handshake") ->
          begin match List.assoc_opt "params" fields with
          | Some (`Assoc parameters) ->
              begin match List.assoc_opt "protocol_version" parameters with
              | Some (`Int version) when version = protocol_version ->
                  `Assoc
                    [
                      ("jsonrpc", `String "2.0");
                      ("id", id);
                      ( "result",
                        `Assoc
                          [
                            ("protocol_version", `Int protocol_version);
                            ( "server",
                              `Assoc
                                [
                                  ("name", `String server_name);
                                  ("version", `String "0.1.0");
                                ] );
                            ( "capabilities",
                              `List
                                (List.map
                                   (fun capability -> `String capability)
                                   capabilities) );
                          ] );
                    ]
              | Some (`Int version) ->
                  error_response ~id ~code:(-32001)
                    ~message:
                      (Printf.sprintf
                         "unsupported protocol version %d; supported version \
                          is %d"
                         version protocol_version)
              | _ ->
                  error_response ~id ~code:(-32602)
                    ~message:"params.protocol_version must be an integer"
              end
          | _ ->
              error_response ~id ~code:(-32602)
                ~message:"handshake params must be an object"
          end
      | Some (`String "2.0"), Some (`String _) ->
          error_response ~id ~code:(-32601) ~message:"method not found"
      | _ ->
          error_response ~id ~code:(-32600) ~message:"invalid JSON-RPC request"
      end
  | _ -> error_response ~id ~code:(-32600) ~message:"invalid JSON-RPC request"

let decode_message ~max_bytes input =
  if String.length input > max_bytes then
    Error (Printf.sprintf "adapter message exceeds the %d byte limit" max_bytes)
  else
    try Ok (Yojson.Safe.from_string input)
    with Yojson.Json_error message -> Error ("invalid JSON: " ^ message)
