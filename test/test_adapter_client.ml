open Deshell

let response ?(extra = []) request result =
  let id = Yojson.Safe.Util.member "id" request in
  `Assoc ([ ("jsonrpc", `String "2.0"); ("id", id); ("result", result) ] @ extra)
  |> Yojson.Safe.to_string

let fake_server ?(capabilities = [ "frontend.detect" ]) handler =
  let exchange raw =
    let request = Yojson.Safe.from_string raw in
    match Yojson.Safe.Util.(request |> member "method" |> to_string) with
    | "deshell.handshake" ->
        Ok
          (response
             ~extra:[ ("future", `Bool true) ]
             request
             (`Assoc
                [
                  ("protocol_version", `Int 1);
                  ( "server",
                    `Assoc
                      [ ("name", `String "fake"); ("version", `String "1.2.3") ]
                  );
                  ( "capabilities",
                    `List (List.map (fun value -> `String value) capabilities)
                  );
                ]))
    | _ -> handler request
  in
  Adapter_client.{ exchange; close = (fun () -> ()) }

let handshake client =
  match Adapter_client.handshake client with
  | Ok server -> server
  | Error message -> Alcotest.fail message

let test_handshake_then_call () =
  let transport =
    fake_server (fun request ->
        Ok
          (response request
             (`Assoc [ ("interpreter", `String "bash"); ("future", `Int 1) ])))
  in
  let client = Adapter_client.create ~max_bytes:4096 transport in
  let server = handshake client in
  Alcotest.(check string) "server" "fake" server.name;
  Alcotest.(check (list string))
    "capabilities" [ "frontend.detect" ] server.capabilities;
  match
    Adapter_client.call client ~method_:"frontend.detect"
      ~params:(`Assoc [ ("path", `String "build.sh") ])
  with
  | Error message -> Alcotest.fail message
  | Ok result ->
      Alcotest.(check string)
        "result" "bash"
        Yojson.Safe.Util.(result |> member "interpreter" |> to_string)

let test_call_before_handshake_is_rejected () =
  let client =
    Adapter_client.create ~max_bytes:4096
      (fake_server (fun _ -> Alcotest.fail "must not exchange"))
  in
  match
    Adapter_client.call client ~method_:"frontend.detect" ~params:(`Assoc [])
  with
  | Ok _ -> Alcotest.fail "call before handshake must fail"
  | Error message ->
      Alcotest.(check bool)
        "diagnostic" true
        (Test_support.contains ~needle:"handshake" message)

let test_unadvertised_capability_is_rejected_locally () =
  let exchanged = ref 0 in
  let transport =
    fake_server ~capabilities:[ "frontend.detect" ] (fun _ ->
        incr exchanged;
        Alcotest.fail "must not call server")
  in
  let client = Adapter_client.create ~max_bytes:4096 transport in
  ignore (handshake client);
  begin match
    Adapter_client.call client ~method_:"frontend.lower" ~params:(`Assoc [])
  with
  | Ok _ -> Alcotest.fail "missing capability must fail"
  | Error message ->
      Alcotest.(check bool)
        "capability" true
        (Test_support.contains ~needle:"capability" message)
  end;
  Alcotest.(check int) "no method exchange" 0 !exchanged

let test_wrong_response_id_is_rejected () =
  let transport =
    fake_server (fun request ->
        let _ = request in
        Ok {|{"jsonrpc":"2.0","id":999,"result":{"interpreter":"sh"}}|})
  in
  let client = Adapter_client.create ~max_bytes:4096 transport in
  ignore (handshake client);
  match
    Adapter_client.call client ~method_:"frontend.detect" ~params:(`Assoc [])
  with
  | Ok _ -> Alcotest.fail "wrong id must fail"
  | Error message ->
      Alcotest.(check bool)
        "id diagnostic" true
        (Test_support.contains ~needle:"response id" message)

let test_rpc_error_and_disconnect_are_attributed () =
  let errored =
    fake_server (fun request ->
        let id = Yojson.Safe.Util.member "id" request in
        Ok
          (Yojson.Safe.to_string
             (`Assoc
                [
                  ("jsonrpc", `String "2.0");
                  ("id", id);
                  ( "error",
                    `Assoc
                      [
                        ("code", `Int (-32010));
                        ("message", `String "parser crashed");
                      ] );
                ])))
  in
  let client = Adapter_client.create ~max_bytes:4096 errored in
  ignore (handshake client);
  begin match
    Adapter_client.call client ~method_:"frontend.detect" ~params:(`Assoc [])
  with
  | Ok _ -> Alcotest.fail "RPC error must fail"
  | Error message ->
      Alcotest.(check bool)
        "code" true
        (Test_support.contains ~needle:"-32010" message);
      Alcotest.(check bool)
        "message" true
        (Test_support.contains ~needle:"parser crashed" message)
  end;
  let disconnected =
    Adapter_client.create ~max_bytes:4096
      Adapter_client.
        { exchange = (fun _ -> Error "adapter disconnected"); close = ignore }
  in
  match Adapter_client.handshake disconnected with
  | Ok _ -> Alcotest.fail "disconnect must fail"
  | Error message ->
      Alcotest.(check string) "transport" "adapter disconnected" message

let test_response_size_limit () =
  let transport =
    fake_server (fun request ->
        Ok
          (response request
             (`Assoc [ ("blob", `String (String.make 5000 'x')) ])))
  in
  let client = Adapter_client.create ~max_bytes:1024 transport in
  ignore (handshake client);
  match
    Adapter_client.call client ~method_:"frontend.detect" ~params:(`Assoc [])
  with
  | Ok _ -> Alcotest.fail "oversized response must fail"
  | Error message ->
      Alcotest.(check bool)
        "limit" true
        (Test_support.contains ~needle:"1024" message)

let () =
  Alcotest.run "Adapter client"
    [
      ( "JSON-RPC conformance",
        [
          Alcotest.test_case "handshake/call" `Quick test_handshake_then_call;
          Alcotest.test_case "handshake required" `Quick
            test_call_before_handshake_is_rejected;
          Alcotest.test_case "capability required" `Quick
            test_unadvertised_capability_is_rejected_locally;
          Alcotest.test_case "response id" `Quick
            test_wrong_response_id_is_rejected;
          Alcotest.test_case "errors/disconnect" `Quick
            test_rpc_error_and_disconnect_are_attributed;
          Alcotest.test_case "response size" `Quick test_response_size_limit;
        ] );
    ]
