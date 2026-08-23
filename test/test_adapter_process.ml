open Deshell

let fixture () =
  match Sys.getenv_opt "DESHELL_TEST_ADAPTER_EXE" with
  | Some value -> value
  | None -> Alcotest.fail "DESHELL_TEST_ADAPTER_EXE is not set"

let connect ?(arguments = []) ?(timeout_seconds = 2.0) ?(max_bytes = 4096) () =
  match
    Adapter_client.connect_process ~program:(fixture ()) ~arguments
      ~timeout_seconds ~max_bytes ()
  with
  | Ok client -> client
  | Error message -> Alcotest.fail message

let handshake client =
  match Adapter_client.handshake client with
  | Ok server -> server
  | Error message -> Alcotest.fail message

let test_process_handshake_and_call () =
  let client = connect () in
  Fun.protect
    ~finally:(fun () -> Adapter_client.close client)
    (fun () ->
      let server = handshake client in
      Alcotest.(check string) "server" "process-fixture" server.name;
      match
        Adapter_client.call client ~method_:"frontend.detect"
          ~params:(`Assoc [ ("path", `String "build.ps1") ])
      with
      | Error message -> Alcotest.fail message
      | Ok result ->
          Alcotest.(check string)
            "official parser" "official-ast"
            Yojson.Safe.Util.(result |> member "parser" |> to_string))

let test_timeout_terminates_adapter () =
  let client = connect ~timeout_seconds:0.5 () in
  ignore (handshake client);
  let started = Unix.gettimeofday () in
  let outcome =
    Adapter_client.call client ~method_:"frontend.hang" ~params:(`Assoc [])
  in
  let elapsed = Unix.gettimeofday () -. started in
  Adapter_client.close client;
  begin match outcome with
  | Ok _ -> Alcotest.fail "hung adapter must time out"
  | Error message ->
      Alcotest.(check bool)
        "timeout diagnostic" true
        (Test_support.contains ~needle:"timed out" message)
  end;
  Alcotest.(check bool) "bounded wait" true (elapsed < 2.0)

let test_early_exit_is_attributed () =
  let client = connect ~arguments:[ "--exit-immediately" ] () in
  let outcome = Adapter_client.handshake client in
  Adapter_client.close client;
  match outcome with
  | Ok _ -> Alcotest.fail "exited adapter must not handshake"
  | Error message ->
      Alcotest.(check bool)
        "disconnect diagnostic" true
        (Test_support.contains ~needle:"disconnected" message)

let test_process_response_limit () =
  let client = connect ~max_bytes:1024 () in
  ignore (handshake client);
  let outcome =
    Adapter_client.call client ~method_:"frontend.large" ~params:(`Assoc [])
  in
  Adapter_client.close client;
  match outcome with
  | Ok _ -> Alcotest.fail "oversized process response must fail"
  | Error message ->
      Alcotest.(check bool)
        "limit diagnostic" true
        (Test_support.contains ~needle:"1024" message)

let () =
  Alcotest.run "Adapter process transport"
    [
      ( "stdio JSON-RPC",
        [
          Alcotest.test_case "handshake/call" `Quick
            test_process_handshake_and_call;
          Alcotest.test_case "timeout" `Quick test_timeout_terminates_adapter;
          Alcotest.test_case "early exit" `Quick test_early_exit_is_attributed;
          Alcotest.test_case "response limit" `Quick test_process_response_limit;
        ] );
    ]
