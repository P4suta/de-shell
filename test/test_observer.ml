open Deshell

let scenario =
  Scenario.
    {
      name = "default";
      args = [ "arg" ];
      environment = [ ("MODE", "test") ];
      fixtures =
        [ { path = "fixture.txt"; contents = "initial"; executable = false } ];
      timeout_ms = 1000;
      expect = { exit_code = None; stdout = None; stderr = None; files = [] };
    }

let observation stdout =
  Observation.
    {
      exit_code = 0;
      stdout;
      stderr = "";
      timed_out = false;
      signal = None;
      processes = [];
      files = [];
      network = [];
    }

let plan () =
  let body =
    Ir.node ~id:"root"
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Exec (Ir.exec [ "printf"; "same" ]))
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ]

let test_independent_snapshots_and_equivalence () =
  Test_support.with_temp_dir @@ fun root ->
  Test_support.write_file (Filename.concat root "build.sh") "printf same\n";
  let workspaces = ref [] in
  let launch _provider (request : Lab.request) =
    workspaces := request.workspace :: !workspaces;
    Alcotest.(check string)
      "fresh fixture" "initial"
      (Test_support.read_file (Filename.concat request.workspace "fixture.txt"));
    Test_support.write_file
      (Filename.concat request.workspace "fixture.txt")
      "mutated";
    if request.interpreter = "deshell" then
      Alcotest.(check bool)
        "scenario argv forwarded" true
        (List.exists (String.equal "arg") request.args
        && List.exists (String.equal "--arg") request.args);
    Ok (observation "same")
  in
  let report =
    Observer.verify ~launch ~provider:Lab.Podman ~root ~entry:"build.sh"
      ~plan:(plan ()) ~scenarios:[ scenario ]
      ~image:
        "lab@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  in
  begin match report with
  | Error message -> Alcotest.fail message
  | Ok report -> Alcotest.(check bool) "verified" true report.verified
  end;
  Alcotest.(check int) "two isolated runs" 2 (List.length !workspaces);
  let left, right =
    match !workspaces with
    | [ left; right ] -> (left, right)
    | _ -> Alcotest.fail "expected two workspaces"
  in
  Alcotest.(check bool) "distinct" false (String.equal left right);
  Alcotest.(check bool) "left cleaned" false (Sys.file_exists left);
  Alcotest.(check bool) "right cleaned" false (Sys.file_exists right)

let test_mismatch_is_returned_not_promoted () =
  Test_support.with_temp_dir @@ fun root ->
  Test_support.write_file (Filename.concat root "build.sh") "printf original\n";
  let launch _provider (request : Lab.request) =
    if request.interpreter = "deshell" then Ok (observation "migrated")
    else Ok (observation "original")
  in
  match
    Observer.verify ~launch ~provider:Lab.Podman ~root ~entry:"build.sh"
      ~plan:(plan ()) ~scenarios:[ scenario ]
      ~image:
        "lab@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  with
  | Error message -> Alcotest.fail message
  | Ok report ->
      Alcotest.(check bool) "not verified" false report.verified;
      begin match report.results with
      | [ Differential.Different comparison ] ->
          Alcotest.(check (list string))
            "stdout difference" [ "stdout" ]
            (List.map Observation.dimension comparison.differences)
      | _ -> Alcotest.fail "expected mismatch"
      end

let test_windows_sandbox_config_is_launched_transactionally () =
  Test_support.with_temp_dir @@ fun root ->
  let workspace = Filename.concat root "workspace" in
  let output = Filename.concat root "output" in
  Unix.mkdir workspace 0o700;
  Unix.mkdir output 0o700;
  let result_path = Filename.concat output "observation.json" in
  let request =
    Lab.
      {
        workspace;
        result_path;
        interpreter = "cmd";
        script = "build.cmd";
        args = [];
        environment = [];
        timeout_ms = 1000;
        network = Deny;
        image = "unused-on-windows";
      }
  in
  let config_path = ref None in
  let execute (process : Runner.process_request) =
    match process.argv with
    | [ executable; path ] ->
        Alcotest.(check bool)
          "Windows Sandbox executable" true
          (Test_support.contains ~needle:"WindowsSandbox.exe" executable);
        Alcotest.(check bool)
          "config exists while launching" true (Sys.file_exists path);
        Alcotest.(check string)
          "config contents" "<Configuration />\n"
          (Test_support.read_file path);
        config_path := Some path;
        Test_support.write_file result_path
          (Observation.encode_string (observation "sandbox"));
        Ok Runner.{ exit_code = 0; stdout = ""; stderr = "" }
    | _ -> Alcotest.fail "unexpected Windows Sandbox invocation"
  in
  begin match
    Observer.launch_windows_config ~execute request "<Configuration />\n"
  with
  | Error message -> Alcotest.fail message
  | Ok value -> Alcotest.(check string) "observation" "sandbox" value.stdout
  end;
  match !config_path with
  | None -> Alcotest.fail "Windows Sandbox was not launched"
  | Some path ->
      Alcotest.(check bool)
        "temporary config removed" false (Sys.file_exists path)

let test_windows_sandbox_failure_removes_config () =
  Test_support.with_temp_dir @@ fun root ->
  let workspace = Filename.concat root "workspace" in
  let output = Filename.concat root "output" in
  Unix.mkdir workspace 0o700;
  Unix.mkdir output 0o700;
  let request =
    Lab.
      {
        workspace;
        result_path = Filename.concat output "observation.json";
        interpreter = "cmd";
        script = "build.cmd";
        args = [];
        environment = [];
        timeout_ms = 1000;
        network = Deny;
        image = "unused-on-windows";
      }
  in
  let config_path = ref None in
  let execute (process : Runner.process_request) =
    config_path := List.nth_opt process.argv 1;
    Ok Runner.{ exit_code = 7; stdout = ""; stderr = "launcher failed" }
  in
  begin match
    Observer.launch_windows_config ~execute request "<Configuration />\n"
  with
  | Ok _ -> Alcotest.fail "a failed Windows Sandbox launch was accepted"
  | Error message ->
      Alcotest.(check bool)
        "exit diagnostic" true
        (Test_support.contains ~needle:"exited 7" message)
  end;
  match !config_path with
  | None -> Alcotest.fail "Windows Sandbox was not invoked"
  | Some path ->
      Alcotest.(check bool) "failed config removed" false (Sys.file_exists path)

let () =
  Alcotest.run "Observer orchestration"
    [
      ( "differential labs",
        [
          Alcotest.test_case "isolated equivalence" `Quick
            test_independent_snapshots_and_equivalence;
          Alcotest.test_case "mismatch" `Quick
            test_mismatch_is_returned_not_promoted;
          Alcotest.test_case "Windows Sandbox launcher" `Quick
            test_windows_sandbox_config_is_launched_transactionally;
          Alcotest.test_case "Windows Sandbox failure cleanup" `Quick
            test_windows_sandbox_failure_removes_config;
        ] );
    ]
