open Deshell

let test_agent executable () =
  Test_support.with_temp_dir @@ fun parent ->
  let workspace = Filename.concat parent "workspace" in
  Unix.mkdir workspace 0o700;
  let script, interpreter, source =
    if Sys.win32 then
      ( "observe.cmd",
        "cmd",
        "@echo off\r\necho agent-output\r\necho data>created.txt\r\n" )
    else
      ( "observe.sh",
        "sh",
        "#!/bin/sh\nprintf 'agent-output\\n'\nprintf data > created.txt\n" )
  in
  Test_support.write_file (Filename.concat workspace script) source;
  let result_path = Filename.concat parent "result.json" in
  let result =
    Test_support.run_process executable
      [
        "--workspace";
        workspace;
        "--result";
        result_path;
        "--timeout-ms";
        "5000";
        "--interpreter";
        interpreter;
        "--script";
        script;
        "--";
      ]
  in
  Alcotest.(check int) "agent exit" 0 result.status;
  Alcotest.(check bool) "result exists" true (Sys.file_exists result_path);
  let observation =
    match Observation.decode_string (Test_support.read_file result_path) with
    | Ok value -> value
    | Error errors -> Alcotest.fail (String.concat "; " errors)
  in
  if observation.exit_code <> 0 then
    Alcotest.failf "script exit=%d; agent stderr=%S; script stderr=%S"
      observation.exit_code result.stderr observation.stderr;
  Alcotest.(check bool)
    "stdout" true
    (Test_support.contains ~needle:"agent-output" observation.stdout);
  Alcotest.(check bool)
    "created file" true
    (List.exists
       (fun (file_effect : Observation.file_effect) ->
         file_effect.path = "created.txt"
         && file_effect.before = None
         && Option.is_some file_effect.after)
       observation.files)

let () =
  let executable =
    match Sys.getenv_opt "DESHELL_TEST_AGENT_EXE" with
    | Some value -> value
    | None -> Alcotest.fail "DESHELL_TEST_AGENT_EXE is not set"
  in
  Alcotest.run "Observer agent executable"
    [
      ( "black box",
        [ Alcotest.test_case "observe process" `Quick (test_agent executable) ]
      );
    ]
