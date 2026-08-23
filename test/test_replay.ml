open Deshell

let request kind key payload =
  Replay.{ kind; key; request_digest = Sha256.hex payload }

let test_record_then_replay () =
  let recorder = Replay.Recorder.create () in
  let time = request Replay.Time "clock:build" "" in
  let random = request Replay.Random "nonce" "16" in
  Replay.Recorder.record recorder time
    Replay.{ status = 0; body = "2030-01-01T00:00:00Z" };
  Replay.Recorder.record recorder random
    Replay.{ status = 0; body = "0011223344556677" };
  let tape = Replay.Recorder.finish recorder in
  let encoded = Replay.encode_string tape in
  let decoded =
    match Replay.decode_string encoded with
    | Ok value -> value
    | Error errors -> Alcotest.fail (String.concat "; " errors)
  in
  let player = Replay.Player.create decoded in
  begin match Replay.Player.next player time with
  | Error message -> Alcotest.fail message
  | Ok response ->
      Alcotest.(check string) "time" "2030-01-01T00:00:00Z" response.body
  end;
  begin match Replay.Player.next player random with
  | Error message -> Alcotest.fail message
  | Ok response ->
      Alcotest.(check string) "random" "0011223344556677" response.body
  end;
  Alcotest.(check (result unit string))
    "fully consumed" (Ok ())
    (Replay.Player.finish player)

let test_request_mismatch_does_not_consume () =
  let expected = request Replay.Network "GET https://example.invalid" "" in
  let tape =
    Replay.
      {
        version = 1;
        exchanges = [ (expected, { status = 200; body = "recorded" }) ];
      }
  in
  let player = Replay.Player.create tape in
  let wrong = request Replay.Network "POST https://example.invalid" "" in
  begin match Replay.Player.next player wrong with
  | Ok _ -> Alcotest.fail "mismatch must fail"
  | Error message ->
      Alcotest.(check bool)
        "diagnostic" true
        (Test_support.contains ~needle:"replay mismatch" message)
  end;
  begin match Replay.Player.next player expected with
  | Error message -> Alcotest.fail message
  | Ok response ->
      Alcotest.(check string) "still available" "recorded" response.body
  end

let test_finish_rejects_unused_exchange () =
  let request = request Replay.Time "clock" "" in
  let tape =
    Replay.
      { version = 1; exchanges = [ (request, { status = 0; body = "value" }) ] }
  in
  let player = Replay.Player.create tape in
  match Replay.Player.finish player with
  | Ok () -> Alcotest.fail "unused nondeterminism must be reported"
  | Error message ->
      Alcotest.(check bool)
        "unused" true
        (Test_support.contains ~needle:"1 unconsumed" message)

let test_secret_response_is_redacted () =
  let recorder = Replay.Recorder.create () in
  let request = request Replay.Network "Authorization: secret-token" "" in
  Replay.Recorder.record ~secret:true recorder request
    Replay.{ status = 200; body = "private-body" };
  let encoded = Replay.Recorder.finish recorder |> Replay.encode_string in
  Alcotest.(check bool)
    "secret absent" false
    (Test_support.contains ~needle:"private-body" encoded
    || Test_support.contains ~needle:"secret-token" encoded);
  Alcotest.(check bool)
    "redaction marker" true
    (Test_support.contains ~needle:"redacted" encoded)

let test_replay_network_backend () =
  let expected = request Replay.Network "GET https://example.invalid" "" in
  let player =
    Replay.Player.create
      Replay.
        {
          version = 1;
          exchanges = [ (expected, { status = 200; body = "recorded-body" }) ];
        }
  in
  let denied _ = Error "unexpected base backend call" in
  let base : Runner.backend =
    {
      execute = denied;
      read_file = denied;
      write_file = (fun ~path:_ ~contents:_ ~append:_ -> Error "unexpected");
      remove_file = denied;
      network_request = (fun ~method_:_ ~uri:_ -> Error "live network denied");
    }
  in
  let backend = Replay.wrap_backend player base in
  begin match
    backend.network_request ~method_:"GET" ~uri:"https://example.invalid"
  with
  | Error message -> Alcotest.fail message
  | Ok body -> Alcotest.(check string) "recorded body" "recorded-body" body
  end;
  Alcotest.(check (result unit string))
    "consumed" (Ok ())
    (Replay.Player.finish player)

let () =
  Alcotest.run "Record/replay contract"
    [
      ( "determinism",
        [
          Alcotest.test_case "record/replay" `Quick test_record_then_replay;
          Alcotest.test_case "mismatch" `Quick
            test_request_mismatch_does_not_consume;
          Alcotest.test_case "unused" `Quick test_finish_rejects_unused_exchange;
          Alcotest.test_case "secret redaction" `Quick
            test_secret_response_is_redacted;
          Alcotest.test_case "runner network backend" `Quick
            test_replay_network_backend;
        ] );
    ]
