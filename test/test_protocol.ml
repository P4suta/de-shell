open Deshell

let handshake ?(version = 1) () =
  `Assoc
    [
      ("jsonrpc", `String "2.0");
      ("id", `Int 7);
      ("method", `String "deshell.handshake");
      ( "params",
        `Assoc
          [
            ("protocol_version", `Int version);
            ( "client",
              `Assoc
                [ ("name", `String "deshell"); ("version", `String "0.1.0") ] );
            ("future_field", `Bool true);
          ] );
    ]

let test_handshake () =
  let response =
    Plugin_protocol.handle_handshake ~server_name:"posix"
      ~capabilities:[ "detect"; "parse"; "lower" ]
      (handshake ())
  in
  match response with
  | `Assoc fields ->
      Alcotest.(check (option string))
        "jsonrpc" (Some "2.0")
        (Option.bind
           (List.assoc_opt "jsonrpc" fields)
           Yojson.Safe.Util.to_string_option);
      let result = List.assoc "result" fields |> Yojson.Safe.Util.to_assoc in
      Alcotest.(check int)
        "protocol" 1
        (List.assoc "protocol_version" result |> Yojson.Safe.Util.to_int)
  | _ -> Alcotest.fail "handshake response must be a JSON-RPC object"

let test_incompatible_protocol () =
  let response =
    Plugin_protocol.handle_handshake ~server_name:"posix" ~capabilities:[]
      (handshake ~version:99 ())
  in
  let error = Yojson.Safe.Util.member "error" response in
  Alcotest.(check int)
    "stable error code" (-32001)
    Yojson.Safe.Util.(error |> member "code" |> to_int)

let test_size_limit () =
  let oversized = String.make 129 'x' in
  match Plugin_protocol.decode_message ~max_bytes:128 oversized with
  | Ok _ -> Alcotest.fail "oversized adapter messages must be rejected"
  | Error message ->
      Alcotest.(check bool)
        "diagnostic" true
        (Test_support.contains ~needle:"128" message)

let () =
  Alcotest.run "Adapter protocol"
    [
      ( "JSON-RPC handshake",
        [
          Alcotest.test_case "compatible and extensible" `Quick test_handshake;
          Alcotest.test_case "incompatible version" `Quick
            test_incompatible_protocol;
          Alcotest.test_case "message size" `Quick test_size_limit;
        ] );
    ]
