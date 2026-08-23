open Deshell

let fake_result ?(exit_code = 0) ?(timed_out = false) ?signal ?(network = []) ()
    =
  Observer_agent.
    {
      exit_code;
      stdout = "stdout";
      stderr = "stderr";
      timed_out;
      signal;
      processes =
        [ Observation.{ argv = [ "tool" ]; exit_code; parent = None } ];
      network;
    }

let test_combines_process_and_filesystem_observation () =
  Test_support.with_temp_dir @@ fun root ->
  Test_support.write_file (Filename.concat root "changed") "before";
  let execute request =
    Alcotest.(check string) "working directory" root request.Observer_agent.cwd;
    Test_support.write_file (Filename.concat root "changed") "after";
    Test_support.write_file (Filename.concat root "created") "new";
    Ok
      (fake_result
         ~network:
           [
             Observation.
               {
                 method_ = "GET";
                 uri = "https://example.invalid";
                 request_digest = Sha256.hex "";
                 response_digest = Sha256.hex "body";
                 status = 200;
               };
           ]
         ())
  in
  match
    Observer_agent.run ~execute ~root ~argv:[ "tool" ]
      ~environment:[ ("MODE", "test") ]
      ~timeout_ms:1000
  with
  | Error message -> Alcotest.fail message
  | Ok observation ->
      Alcotest.(check int) "exit" 0 observation.exit_code;
      Alcotest.(check string) "stdout" "stdout" observation.stdout;
      Alcotest.(check (list string))
        "file paths" [ "changed"; "created" ]
        (List.map
           (fun (file_effect : Observation.file_effect) -> file_effect.path)
           observation.files);
      Alcotest.(check int) "processes" 1 (List.length observation.processes);
      Alcotest.(check int) "network" 1 (List.length observation.network)

let test_timeout_and_signal_are_preserved () =
  Test_support.with_temp_dir @@ fun root ->
  let execute _ =
    Ok (fake_result ~exit_code:124 ~timed_out:true ~signal:9 ())
  in
  match
    Observer_agent.run ~execute ~root ~argv:[ "slow" ] ~environment:[]
      ~timeout_ms:10
  with
  | Error message -> Alcotest.fail message
  | Ok observation ->
      Alcotest.(check bool) "timed out" true observation.timed_out;
      Alcotest.(check (option int)) "signal" (Some 9) observation.signal;
      Alcotest.(check int) "exit" 124 observation.exit_code

let test_invalid_requests_do_not_execute () =
  Test_support.with_temp_dir @@ fun root ->
  let called = ref false in
  let execute _ =
    called := true;
    Ok (fake_result ())
  in
  begin match
    Observer_agent.run ~execute ~root ~argv:[] ~environment:[] ~timeout_ms:1000
  with
  | Ok _ -> Alcotest.fail "empty argv must fail"
  | Error message ->
      Alcotest.(check bool)
        "argv diagnostic" true
        (Test_support.contains ~needle:"argv" message)
  end;
  begin match
    Observer_agent.run ~execute ~root ~argv:[ "tool" ] ~environment:[]
      ~timeout_ms:0
  with
  | Ok _ -> Alcotest.fail "zero timeout must fail"
  | Error message ->
      Alcotest.(check bool)
        "timeout diagnostic" true
        (Test_support.contains ~needle:"timeout" message)
  end;
  Alcotest.(check bool) "never executed" false !called

let test_invocation_codec_round_trip_and_rejects_corruption () =
  let invocation =
    Observer_agent.
      {
        workspace = "C:\\workspace";
        result_path = "C:\\output\\result.json";
        timeout_ms = 1234;
        interpreter = "powershell";
        script = "script.ps1";
        args = [ "hello & goodbye"; "%PATH%" ];
        environment = [ ("TOKEN", "sensitive=value") ];
      }
  in
  let encoded = Observer_agent.encode_invocation invocation in
  Alcotest.(check bool)
    "command-line alphabet" true
    (String.for_all
       (function
         | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '+' | '/' | '=' -> true
         | _ -> false)
       encoded);
  begin match Observer_agent.decode_invocation encoded with
  | Error message -> Alcotest.fail message
  | Ok decoded ->
      Alcotest.(check string) "workspace" invocation.workspace decoded.workspace;
      Alcotest.(check (list string)) "args" invocation.args decoded.args;
      Alcotest.(check (list (pair string string)))
        "environment" invocation.environment decoded.environment
  end;
  match Observer_agent.decode_invocation "not-base64!" with
  | Ok _ -> Alcotest.fail "corrupt observer request was accepted"
  | Error _ -> ()

let () =
  Alcotest.run "Observer agent core"
    [
      ( "observation",
        [
          Alcotest.test_case "combined effects" `Quick
            test_combines_process_and_filesystem_observation;
          Alcotest.test_case "timeout/signal" `Quick
            test_timeout_and_signal_are_preserved;
          Alcotest.test_case "invalid request" `Quick
            test_invalid_requests_do_not_execute;
          Alcotest.test_case "invocation codec" `Quick
            test_invocation_codec_round_trip_and_rejects_corruption;
        ] );
    ]
