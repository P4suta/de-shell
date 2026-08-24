let test_version executable () =
  let result = Test_support.run_process executable [ "--version" ] in
  Alcotest.(check int) "exit" 0 result.status;
  Alcotest.(check bool)
    "version" true
    (Test_support.contains ~needle:"0.1.0" result.stdout)

let test_init_analyze_check executable () =
  Test_support.with_temp_dir @@ fun root ->
  let initialized =
    Test_support.run_process executable [ "init"; "--root"; root ]
  in
  Alcotest.(check int) "init exit" 0 initialized.status;
  Alcotest.(check bool)
    "project config" true
    (Sys.file_exists (Filename.concat root ".deshell/project.toml"));
  Alcotest.(check bool)
    "scenario" true
    (Sys.file_exists (Filename.concat root ".deshell/scenarios/default.toml"));
  Alcotest.(check bool)
    "lock" true
    (Sys.file_exists (Filename.concat root "deshell.lock"));
  Test_support.write_file
    (Filename.concat root "hello.sh")
    "#!/bin/sh\nprintf 'hello\\n'\n";
  let analyzed =
    Test_support.run_process executable
      [ "analyze"; "--root"; root; "--entry"; "hello.sh" ]
  in
  Alcotest.(check int) "analyze exit" 0 analyzed.status;
  let plan_path = Filename.concat root ".deshell/plan.json" in
  let evidence_path = Filename.concat root ".deshell/evidence.json" in
  Alcotest.(check bool) "plan" true (Sys.file_exists plan_path);
  Alcotest.(check bool) "evidence" true (Sys.file_exists evidence_path);
  let plan = Yojson.Safe.from_file plan_path in
  Alcotest.(check int)
    "canonical schema" 3
    Yojson.Safe.Util.(plan |> member "schema_version" |> to_int);
  let checked =
    Test_support.run_process executable [ "check"; "--root"; root ]
  in
  Alcotest.(check int) "check exit" 0 checked.status

let test_configured_entrypoint_is_used executable () =
  Test_support.with_temp_dir @@ fun root ->
  let initialized =
    Test_support.run_process executable [ "init"; "--root"; root ]
  in
  Alcotest.(check int) "init" 0 initialized.status;
  Test_support.write_file
    (Filename.concat root "configured.sh")
    "printf configured\n";
  let config = Filename.concat root ".deshell/project.toml" in
  Test_support.write_file config
    (String.concat "\n"
       [
         "version = 1";
         "entrypoints = [\"configured.sh\"]";
         "";
         "[policy]";
         "host_write = \"deny\"";
         "network = \"deny\"";
         "unknown_interpreter = \"trace-only\"";
         "";
         "[sandbox]";
         "mode = \"disposable\"";
         "";
         "[export]";
         "strict = true";
         "bridge = false";
         "";
       ]);
  let analyzed =
    Test_support.run_process executable [ "analyze"; "--root"; root ]
  in
  Alcotest.(check int) "configured analyze" 0 analyzed.status;
  Alcotest.(check bool)
    "plan created" true
    (Sys.file_exists (Filename.concat root ".deshell/plan.json"))

let test_unknown_interpreter_reject_policy executable () =
  Test_support.with_temp_dir @@ fun root ->
  let initialized =
    Test_support.run_process executable [ "init"; "--root"; root ]
  in
  Alcotest.(check int) "init" 0 initialized.status;
  let config = Filename.concat root ".deshell/project.toml" in
  let source = Test_support.read_file config in
  let configured =
    let needle = {|unknown_interpreter = "trace-only"|} in
    let replacement = {|unknown_interpreter = "reject"|} in
    match String.index_opt source 't' with
    | None -> Alcotest.fail "default policy fixture is missing"
    | Some _ ->
        let rec replace index =
          if index + String.length needle > String.length source then
            Alcotest.fail "default unknown_interpreter policy is missing"
          else if String.sub source index (String.length needle) = needle then
            String.sub source 0 index ^ replacement
            ^ String.sub source
                (index + String.length needle)
                (String.length source - index - String.length needle)
          else replace (index + 1)
        in
        replace 0
  in
  Test_support.write_file config configured;
  Test_support.write_file (Filename.concat root "build.custom") "do something\n";
  let analyzed =
    Test_support.run_process executable
      [ "analyze"; "--root"; root; "--entry"; "build.custom" ]
  in
  Alcotest.(check bool) "analysis rejected" true (analyzed.status <> 0);
  Alcotest.(check bool)
    "policy diagnostic" true
    (Test_support.contains ~needle:"unknown interpreter" analyzed.stderr)

let test_scan_json executable () =
  Test_support.with_temp_dir @@ fun root ->
  Test_support.write_file (Filename.concat root "script.sh") "echo scan\n";
  let result =
    Test_support.run_process executable
      [ "scan"; "--root"; root; "--format"; "json" ]
  in
  Alcotest.(check int) "scan exit" 0 result.status;
  match Yojson.Safe.from_string result.stdout with
  | `List findings ->
      Alcotest.(check int) "one finding" 1 (List.length findings)
  | _ -> Alcotest.fail "scan --format json must emit a JSON array"

let initialize_and_analyze executable root entry source =
  let initialized =
    Test_support.run_process executable [ "init"; "--root"; root ]
  in
  Alcotest.(check int) "init" 0 initialized.status;
  Test_support.write_file (Filename.concat root entry) source;
  let analyzed =
    Test_support.run_process executable
      [ "analyze"; "--root"; root; "--entry"; entry ]
  in
  Alcotest.(check int) "analyze" 0 analyzed.status

let test_analyze_persists_typed_powershell_inputs executable () =
  Test_support.with_temp_dir @@ fun root ->
  let command =
    if Sys.win32 then "& 'cmd.exe' '/d' '/s' '/c' 'echo' $Name $Count\n"
    else "& '/bin/echo' $Name $Count\n"
  in
  let source =
    "[CmdletBinding()]\n\
     param(\n\
    \  [Parameter(Mandatory = $true, Position = 0)]\n\
    \  [string] $Name,\n\
    \  [int] $Count = 2\n\
     )\n" ^ command
  in
  initialize_and_analyze executable root "typed.ps1" source;
  let plan =
    Yojson.Safe.from_file (Filename.concat root ".deshell/plan.json")
  in
  let task = Yojson.Safe.Util.(plan |> member "tasks" |> to_list |> List.hd) in
  let inputs = Yojson.Safe.Util.(task |> member "inputs" |> to_list) in
  Alcotest.(check (list string))
    "persisted typed inputs" [ "Name"; "Count" ]
    (List.map
       Yojson.Safe.Util.(fun input -> input |> member "name" |> to_string)
       inputs);
  Alcotest.(check string)
    "PowerShell invocation style" "powershell"
    Yojson.Safe.Util.(
      task |> member "invocation" |> member "style" |> to_string);
  Alcotest.(check bool)
    "PowerShell common parameters persisted" true
    Yojson.Safe.Util.(
      task |> member "invocation"
      |> member "accepts_common_parameters"
      |> to_bool);
  Alcotest.(check (list string))
    "task inputs are not host environment" []
    (Yojson.Safe.Util.(task |> member "environment" |> to_list)
    |> List.map Yojson.Safe.Util.to_string);
  let ran =
    Test_support.run_process executable
      [
        "run";
        "--root";
        root;
        "--";
        "artifact";
        "-Count";
        "3";
        "-Verbose:$false";
      ]
  in
  if ran.status <> 0 then
    Alcotest.failf "typed PowerShell CLI run failed: %s"
      (String.trim ran.stderr);
  Alcotest.(check bool)
    "typed PowerShell CLI output" true
    (Test_support.contains ~needle:"artifact 3" ran.stdout);
  let missing = Test_support.run_process executable [ "run"; "--root"; root ] in
  Alcotest.(check bool)
    "missing mandatory input rejected" true (missing.status <> 0);
  Alcotest.(check bool)
    "mandatory diagnostic" true
    (Test_support.contains ~needle:"missing mandatory" missing.stderr)

let test_check_rejects_tampered_evidence executable () =
  Test_support.with_temp_dir @@ fun root ->
  initialize_and_analyze executable root "digest.sh" "printf digest\n";
  let evidence_path = Filename.concat root ".deshell/evidence.json" in
  let evidence = Yojson.Safe.from_file evidence_path in
  let tampered =
    match evidence with
    | `Assoc fields ->
        `Assoc
          (("plan_digest", `String (String.make 64 '0'))
          :: List.remove_assoc "plan_digest" fields)
    | _ -> Alcotest.fail "evidence must be an object"
  in
  Test_support.write_file evidence_path
    (Yojson.Safe.pretty_to_string tampered ^ "\n");
  let checked =
    Test_support.run_process executable [ "check"; "--root"; root ]
  in
  Alcotest.(check bool) "check fails" true (checked.status <> 0);
  Alcotest.(check bool)
    "digest diagnostic" true
    (Test_support.contains ~needle:"digest" checked.stderr)

let test_check_rejects_source_drift executable () =
  Test_support.with_temp_dir @@ fun root ->
  let source_path = Filename.concat root "drift.sh" in
  initialize_and_analyze executable root "drift.sh" "printf before\n";
  Test_support.write_file source_path "printf after\n";
  let checked =
    Test_support.run_process executable [ "check"; "--root"; root ]
  in
  Alcotest.(check bool) "check fails" true (checked.status <> 0);
  Alcotest.(check bool)
    "source diagnostic" true
    (Test_support.contains ~needle:"source digest" checked.stderr)

let test_check_accepts_evidence_extensions executable () =
  Test_support.with_temp_dir @@ fun root ->
  initialize_and_analyze executable root "future.sh" "printf future\n";
  let evidence_path = Filename.concat root ".deshell/evidence.json" in
  let evidence = Yojson.Safe.from_file evidence_path in
  let extended =
    match evidence with
    | `Assoc fields ->
        let nodes =
          match List.assoc_opt "nodes" fields with
          | Some (`List nodes) ->
              List.map
                (function
                  | `Assoc node_fields ->
                      let guarantee =
                        match List.assoc_opt "guarantee" node_fields with
                        | Some (`Assoc guarantee_fields) ->
                            `Assoc
                              (("future_evidence", `Bool true)
                              :: guarantee_fields)
                        | _ -> Alcotest.fail "node guarantee must be an object"
                      in
                      `Assoc
                        (("guarantee", guarantee)
                        :: List.remove_assoc "guarantee" node_fields)
                  | _ -> Alcotest.fail "evidence node must be an object")
                nodes
          | _ -> Alcotest.fail "evidence nodes must be an array"
        in
        `Assoc
          (("future_document", `String "accepted")
          :: ("nodes", `List nodes)
          :: List.remove_assoc "nodes" fields)
    | _ -> Alcotest.fail "evidence must be an object"
  in
  Test_support.write_file evidence_path
    (Yojson.Safe.pretty_to_string extended ^ "\n");
  let checked =
    Test_support.run_process executable [ "check"; "--root"; root ]
  in
  Alcotest.(check int) "extension accepted" 0 checked.status

let test_rewrite_preview_and_apply executable () =
  Test_support.with_temp_dir @@ fun root ->
  let path = Filename.concat root "legacy.sh" in
  Test_support.write_file path "echo `printf hi`\n";
  let preview =
    Test_support.run_process executable
      [ "rewrite"; "--root"; root; "--entry"; "legacy.sh"; "--equivalent" ]
  in
  Alcotest.(check int) "preview exit" 0 preview.status;
  Alcotest.(check bool)
    "diff" true
    (Test_support.contains ~needle:"$(printf hi)" preview.stdout);
  Alcotest.(check string)
    "preview is non-mutating" "echo `printf hi`\n"
    (Test_support.read_file path);
  let applied =
    Test_support.run_process executable
      [
        "rewrite";
        "--root";
        root;
        "--entry";
        "legacy.sh";
        "--equivalent";
        "--apply";
      ]
  in
  Alcotest.(check int) "apply exit" 0 applied.status;
  Alcotest.(check string)
    "applied content" "echo $(printf hi)\n"
    (Test_support.read_file path)

let test_run_and_export executable () =
  Test_support.with_temp_dir @@ fun root ->
  let source =
    if Sys.win32 then "cmd /d /s /c 'echo cli-run'\n" else "printf cli-run\n"
  in
  initialize_and_analyze executable root "run.sh" source;
  let ran = Test_support.run_process executable [ "run"; "--root"; root ] in
  Alcotest.(check int) "run exit" 0 ran.status;
  Alcotest.(check bool)
    "run stdout" true
    (Test_support.contains ~needle:"cli-run" ran.stdout);
  let exported =
    Test_support.run_process executable
      [ "export"; "--root"; root; "--target"; "cwl" ]
  in
  Alcotest.(check int) "export exit" 0 exported.status;
  Alcotest.(check string)
    "CWL output" "v1.2"
    Yojson.Safe.Util.(
      Yojson.Safe.from_string exported.stdout
      |> member "cwlVersion" |> to_string)

let test_run_uses_project_root_as_process_cwd executable () =
  Test_support.with_temp_dir @@ fun root ->
  let source = if Sys.win32 then "@cmd.exe /d /s /c cd\n" else "/bin/pwd\n" in
  initialize_and_analyze executable root
    (if Sys.win32 then "cwd.cmd" else "cwd.sh")
    source;
  let ran = Test_support.run_process executable [ "run"; "--root"; root ] in
  if ran.status <> 0 then
    Alcotest.failf "run exited %d: %s" ran.status (String.trim ran.stderr);
  Alcotest.(check string)
    "process cwd" (Unix.realpath root)
    (Unix.realpath (String.trim ran.stdout))

let test_analyze_declares_and_run_inherits_environment executable () =
  Test_support.with_temp_dir @@ fun root ->
  let variable = "DESHELL_CLI_API_TOKEN" in
  let expected = "inherited-through-typed-ir" in
  let previous = Sys.getenv_opt variable in
  Fun.protect
    ~finally:(fun () ->
      Unix.putenv variable (Option.value ~default:"" previous))
    (fun () ->
      Unix.putenv variable expected;
      let command =
        if Sys.win32 then
          Printf.sprintf "cmd.exe /d /s /c echo \"$%s\"" variable
        else Printf.sprintf "/bin/echo \"$%s\"" variable
      in
      initialize_and_analyze executable root "environment.sh"
        ("#!/bin/sh\nset -eu\n" ^ command ^ "\n");
      let plan_path = Filename.concat root ".deshell/plan.json" in
      let plan = Yojson.Safe.from_file plan_path in
      let task =
        Yojson.Safe.Util.(plan |> member "tasks" |> to_list |> List.hd)
      in
      let environment =
        Yojson.Safe.Util.(task |> member "environment" |> to_list)
        |> List.map Yojson.Safe.Util.to_string
      in
      Alcotest.(check (list string))
        "declared environment" [ variable ] environment;
      let secrets =
        Yojson.Safe.Util.(task |> member "secrets" |> to_list)
        |> List.map Yojson.Safe.Util.to_string
      in
      Alcotest.(check (list string)) "classified secret" [ variable ] secrets;
      let run arguments =
        let result =
          Test_support.run_process executable
            ([ "run"; "--root"; root ] @ arguments)
        in
        if result.status <> 0 then
          Alcotest.failf "run exited %d: %s" result.status
            (String.trim result.stderr);
        Alcotest.(check string)
          "inherited value" expected
          (String.trim result.stdout)
      in
      run [];
      let evidence =
        Yojson.Safe.from_file (Filename.concat root ".deshell/evidence.json")
      in
      let exec_id =
        Yojson.Safe.Util.(evidence |> member "nodes" |> to_list)
        |> List.find_map (fun value ->
            match
              Yojson.Safe.Util.
                ( value |> member "operation" |> to_string,
                  value |> member "id" |> to_string )
            with
            | "exec", id -> Some id
            | _ -> None)
        |> Option.get
      in
      run [ "--node"; exec_id ])

let test_trace_only_analysis_and_bridge executable () =
  Test_support.with_temp_dir @@ fun root ->
  initialize_and_analyze executable root "build.ps1" "Write-Output $env:VALUE\n";
  let plan =
    Yojson.Safe.from_file (Filename.concat root ".deshell/plan.json")
    |> Yojson.Safe.to_string
  in
  Alcotest.(check bool)
    "PowerShell capsule" true
    (Test_support.contains ~needle:"powershell" plan
    && Test_support.contains ~needle:"residual" plan);
  let strict =
    Test_support.run_process executable
      [ "export"; "--root"; root; "--target"; "cwl" ]
  in
  Alcotest.(check bool) "strict rejected" true (strict.status <> 0);
  let bridge =
    Test_support.run_process executable
      [ "export"; "--root"; root; "--target"; "cwl"; "--bridge" ]
  in
  Alcotest.(check int) "bridge exit" 0 bridge.status;
  Alcotest.(check bool)
    "bridge explicit" true
    (Test_support.contains ~needle:"deshell" bridge.stdout)

let test_modernize_preview_and_apply executable () =
  Test_support.with_temp_dir @@ fun root ->
  let path = Filename.concat root "modern.sh" in
  Test_support.write_file path "#!/bin/sh\necho modern\n";
  let preview =
    Test_support.run_process executable
      [ "modernize"; "--root"; root; "--profile"; "secure" ]
  in
  Alcotest.(check int) "preview exit" 0 preview.status;
  Alcotest.(check bool)
    "preview contains change" true
    (Test_support.contains ~needle:"set -eu" preview.stdout);
  Alcotest.(check string)
    "preview does not mutate" "#!/bin/sh\necho modern\n"
    (Test_support.read_file path);
  let applied =
    Test_support.run_process executable
      [ "modernize"; "--root"; root; "--profile"; "secure"; "--apply" ]
  in
  Alcotest.(check int) "apply exit" 0 applied.status;
  Alcotest.(check string)
    "explicitly applied" "#!/bin/sh\nset -eu\necho modern\n"
    (Test_support.read_file path)

let test_modernize_applies_repository_as_one_batch executable () =
  Test_support.with_temp_dir @@ fun root ->
  let first = Filename.concat root "first.sh" in
  let second = Filename.concat root "second.sh" in
  Test_support.write_file first "#!/bin/sh\necho first\n";
  Test_support.write_file second "#!/bin/sh\necho second\n";
  let applied =
    Test_support.run_process executable
      [ "modernize"; "--root"; root; "--profile"; "secure"; "--apply" ]
  in
  Alcotest.(check int) "apply" 0 applied.status;
  List.iter
    (fun path ->
      Alcotest.(check bool)
        "strict mode applied" true
        (Test_support.contains ~needle:"set -eu" (Test_support.read_file path)))
    [ first; second ]

let test_verify_explain_and_migrate executable () =
  Test_support.with_temp_dir @@ fun root ->
  initialize_and_analyze executable root "build.sh" "printf migrate\n";
  let verified =
    Test_support.run_process executable [ "verify"; "--root"; root ]
  in
  Alcotest.(check int) "verify exit" 0 verified.status;
  Alcotest.(check bool)
    "coverage" true
    (Test_support.contains ~needle:"formal=" verified.stdout);
  let explained =
    Test_support.run_process executable [ "explain"; "--root"; root ]
  in
  Alcotest.(check int) "explain exit" 0 explained.status;
  Alcotest.(check bool)
    "node count" true
    (Test_support.contains ~needle:"nodes:" explained.stdout);
  let preview =
    Test_support.run_process executable
      [ "migrate"; "--root"; root; "--entry"; "build.sh"; "--target"; "nu" ]
  in
  Alcotest.(check int) "migrate preview" 0 preview.status;
  Alcotest.(check bool)
    "Nu preview" true
    (Test_support.contains ~needle:"export def main" preview.stdout);
  Alcotest.(check bool)
    "preview does not write export" false
    (Sys.file_exists (Filename.concat root "deshell.nu"));
  let applied =
    Test_support.run_process executable
      [
        "migrate";
        "--root";
        root;
        "--entry";
        "build.sh";
        "--target";
        "nu";
        "--apply";
      ]
  in
  Alcotest.(check int) "migrate apply" 0 applied.status;
  Alcotest.(check bool)
    "Nu artifact" true
    (Sys.file_exists (Filename.concat root "deshell.nu"))

let test_migrate_replaces_callsites_in_artifact_transaction executable () =
  Test_support.with_temp_dir @@ fun root ->
  Unix.mkdir (Filename.concat root "scripts") 0o700;
  initialize_and_analyze executable root "scripts/build.sh" "printf build\n";
  let makefile = Filename.concat root "Makefile" in
  Test_support.write_file makefile "build:\n\t./scripts/build.sh\n";
  let preview =
    Test_support.run_process executable
      [
        "migrate";
        "--root";
        root;
        "--entry";
        "scripts/build.sh";
        "--target";
        "nu";
      ]
  in
  Alcotest.(check int) "preview" 0 preview.status;
  Alcotest.(check bool)
    "callsite preview" true
    (Test_support.contains ~needle:"deshell run" preview.stdout);
  Alcotest.(check string)
    "preview leaves caller" "build:\n\t./scripts/build.sh\n"
    (Test_support.read_file makefile);
  Alcotest.(check bool)
    "preview leaves artifact absent" false
    (Sys.file_exists (Filename.concat root "deshell.nu"));
  let applied =
    Test_support.run_process executable
      [
        "migrate";
        "--root";
        root;
        "--entry";
        "scripts/build.sh";
        "--target";
        "nu";
        "--apply";
      ]
  in
  Alcotest.(check int) "apply" 0 applied.status;
  Alcotest.(check bool)
    "artifact created" true
    (Sys.file_exists (Filename.concat root "deshell.nu"));
  Alcotest.(check string)
    "caller patched atomically" "build:\n\tdeshell run\n"
    (Test_support.read_file makefile)

let test_observe_records_unavailable_evidence executable () =
  Test_support.with_temp_dir @@ fun root ->
  initialize_and_analyze executable root "observe.sh" "printf observe\n";
  let migrated =
    Test_support.run_process executable
      [
        "migrate";
        "--root";
        root;
        "--entry";
        "observe.sh";
        "--observe";
        "--target";
        "internal";
      ]
  in
  Alcotest.(check int)
    "migration continues with reduced guarantee" 0 migrated.status;
  let evidence =
    Yojson.Safe.from_file (Filename.concat root ".deshell/evidence.json")
  in
  let observation = Yojson.Safe.Util.member "observation" evidence in
  Alcotest.(check bool)
    "requested" true
    Yojson.Safe.Util.(observation |> member "requested" |> to_bool);
  Alcotest.(check string)
    "status" "unavailable"
    Yojson.Safe.Util.(observation |> member "status" |> to_string);
  Alcotest.(check bool)
    "reason recorded" true
    (Yojson.Safe.Util.(observation |> member "reason" |> to_string) <> "")

let test_check_rejects_malformed_observation_evidence executable () =
  Test_support.with_temp_dir @@ fun root ->
  initialize_and_analyze executable root "observation.sh" "printf ok\n";
  let evidence_path = Filename.concat root ".deshell/evidence.json" in
  let evidence = Yojson.Safe.from_file evidence_path in
  let malformed =
    match evidence with
    | `Assoc fields ->
        `Assoc
          (( "observation",
             `Assoc
               [ ("requested", `Bool true); ("status", `String "verified") ] )
          :: fields)
    | _ -> Alcotest.fail "evidence must be an object"
  in
  Test_support.write_file evidence_path
    (Yojson.Safe.pretty_to_string malformed ^ "\n");
  let checked =
    Test_support.run_process executable [ "check"; "--root"; root ]
  in
  Alcotest.(check bool)
    "malformed observation rejected" true (checked.status <> 0);
  Alcotest.(check bool)
    "observation diagnostic" true
    (Test_support.contains ~needle:"observation" checked.stderr)

let () =
  let executable =
    match Sys.getenv_opt "DESHELL_TEST_EXE" with
    | Some value -> value
    | None -> Alcotest.fail "DESHELL_TEST_EXE is not set"
  in
  Alcotest.run "CLI acceptance"
    [
      ( "public commands",
        [
          Alcotest.test_case "version" `Quick (test_version executable);
          Alcotest.test_case "init analyze check" `Quick
            (test_init_analyze_check executable);
          Alcotest.test_case "configured entrypoint" `Quick
            (test_configured_entrypoint_is_used executable);
          Alcotest.test_case "typed PowerShell inputs" `Quick
            (test_analyze_persists_typed_powershell_inputs executable);
          Alcotest.test_case "unknown interpreter policy" `Quick
            (test_unknown_interpreter_reject_policy executable);
          Alcotest.test_case "tampered evidence" `Quick
            (test_check_rejects_tampered_evidence executable);
          Alcotest.test_case "source drift" `Quick
            (test_check_rejects_source_drift executable);
          Alcotest.test_case "evidence extensions" `Quick
            (test_check_accepts_evidence_extensions executable);
          Alcotest.test_case "scan json" `Quick (test_scan_json executable);
          Alcotest.test_case "rewrite preview/apply" `Quick
            (test_rewrite_preview_and_apply executable);
          Alcotest.test_case "run and export" `Quick
            (test_run_and_export executable);
          Alcotest.test_case "run project cwd" `Quick
            (test_run_uses_project_root_as_process_cwd executable);
          Alcotest.test_case "environment inheritance" `Quick
            (test_analyze_declares_and_run_inherits_environment executable);
          Alcotest.test_case "trace-only and bridge" `Quick
            (test_trace_only_analysis_and_bridge executable);
          Alcotest.test_case "modernize preview/apply" `Quick
            (test_modernize_preview_and_apply executable);
          Alcotest.test_case "modernize repository transaction" `Quick
            (test_modernize_applies_repository_as_one_batch executable);
          Alcotest.test_case "verify explain migrate" `Quick
            (test_verify_explain_and_migrate executable);
          Alcotest.test_case "migrate callsite transaction" `Quick
            (test_migrate_replaces_callsites_in_artifact_transaction executable);
          Alcotest.test_case "observe evidence" `Quick
            (test_observe_records_unavailable_evidence executable);
          Alcotest.test_case "malformed observation evidence" `Quick
            (test_check_rejects_malformed_observation_evidence executable);
        ] );
    ]
