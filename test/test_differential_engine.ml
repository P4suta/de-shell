open Deshell

let scenario name =
  Scenario.
    {
      name;
      args = [];
      environment = [];
      fixtures = [];
      timeout_ms = 1000;
      expect = { exit_code = None; stdout = None; stderr = None; files = [] };
    }

let observation output =
  Observation.
    {
      exit_code = 0;
      stdout = output;
      stderr = "";
      timed_out = false;
      signal = None;
      processes = [];
      files = [];
      network = [];
    }

let process_observation (result : Runner.process_result) =
  Observation.
    {
      exit_code = result.exit_code;
      stdout = result.stdout;
      stderr = result.stderr;
      timed_out = false;
      signal = None;
      processes = [];
      files = [];
      network = [];
    }

let posix_shell () =
  match Sys.getenv_opt "DESHELL_POSIX_SHELL" with
  | Some path when String.trim path <> "" -> path
  | _ when not Sys.win32 -> "sh"
  | _ ->
      let under variable suffix =
        Option.map
          (fun root -> Filename.concat root suffix)
          (Sys.getenv_opt variable)
      in
      let candidates =
        [
          under "ProgramFiles" "Git/bin/sh.exe";
          under "ProgramW6432" "Git/bin/sh.exe";
          under "LOCALAPPDATA" "Programs/Git/bin/sh.exe";
        ]
        |> List.filter_map Fun.id
      in
      begin match List.find_opt Sys.file_exists candidates with
      | Some path -> path
      | None ->
          Alcotest.fail
            "official POSIX differential test requires sh; set \
             DESHELL_POSIX_SHELL"
      end

let quote_posix_word value =
  "'" ^ String.concat "'\"'\"'" (String.split_on_char '\'' value) ^ "'"

let posix_exec_source argv =
  "exec " ^ String.concat " " (List.map quote_posix_word argv)

let verify_posix_source ~path ~source scenarios =
  let lowered = Posix_frontend.lower ~path source in
  if Posix_frontend.has_residual lowered.root then
    Alcotest.failf "%s unexpectedly stayed residual" path;
  let plan =
    Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:lowered.root () ]
  in
  let shell = posix_shell () in
  let migrated_backend : Runner.backend =
    {
      execute =
        (fun request ->
          let request =
            if Sys.win32 then
              {
                request with
                Runner.argv = [ shell; "-c"; posix_exec_source request.argv ];
              }
            else request
          in
          Process_backend.execute request);
      read_file = (fun _ -> Error "unused");
      write_file = (fun ~path:_ ~contents:_ ~append:_ -> Error "unused");
      remove_file = (fun _ -> Error "unused");
      network_request = (fun ~method_:_ ~uri:_ -> Error "unused");
    }
  in
  let observations = ref [] in
  let original scenario =
    Process_backend.execute
      Runner.
        {
          argv = [ shell; "-c"; source; path ] @ scenario.Scenario.args;
          environment = scenario.environment;
          working_directory = None;
          stdin = "";
        }
    |> Result.map process_observation
    |> Result.map (fun observation ->
        observations :=
          ("original", scenario.Scenario.name, observation) :: !observations;
        observation)
  in
  let migrated scenario =
    Runner.run_plan_with_inputs ~backend:migrated_backend
      ~policy:Runner.default_policy ~inputs:[] ~arguments:scenario.Scenario.args
      plan
    |> Result.map (fun result ->
        let observation =
          process_observation
            Runner.
              {
                exit_code = result.exit_code;
                stdout = result.stdout;
                stderr = result.stderr;
              }
        in
        observations :=
          ("migrated", scenario.Scenario.name, observation) :: !observations;
        observation)
  in
  let report = Differential.run ~scenarios ~original ~migrated in
  if not report.verified then begin
    let observed =
      !observations |> List.rev
      |> List.map (fun (side, name, observation) ->
          Printf.sprintf "%s/%s=%s" side name
            (Observation.encode_string observation))
      |> String.concat "\n"
    in
    let outcomes =
      report.results |> List.map Differential.outcome_to_yojson |> fun values ->
      `List values |> Yojson.Safe.to_string
    in
    Alcotest.failf "%s official shell equivalence failed:\n%s\noutcomes=%s" path
      observed outcomes
  end;
  report

let test_posix_branch_state_matches_official_shell () =
  let source =
    "#!/bin/sh\n\
     set -eu\n\
     if test \"$1\" = release\n\
     then\n\
     mode=release\n\
     else\n\
     mode=debug\n\
     fi\n\
     printf '%s\\n' \"$mode\"\n"
  in
  let scenarios =
    [
      { (scenario "release") with args = [ "release" ] };
      { (scenario "debug") with args = [ "other" ] };
    ]
  in
  let report = verify_posix_source ~path:"branch-state.sh" ~source scenarios in
  Alcotest.(check int) "both branch scenarios" 2 (List.length report.results)

let test_posix_safe_unquoted_matches_official_shell () =
  let source = "#!/bin/sh\nset -eu\nvalue=ok\nprintf '<%s>\\n' $value\n" in
  let report =
    verify_posix_source ~path:"static-unquoted.sh" ~source
      [ scenario "static literal" ]
  in
  Alcotest.(check int) "static scenario" 1 (List.length report.results)

let test_posix_command_capture_matches_official_shell () =
  let source =
    "#!/bin/sh\n\
     set -eu\n\
     captured=$(printf '(%s)\\n\\n' \"$(printf '<%s>' \"$1\")\")\n\
     printf '<%s>\\n' \"$captured\"\n"
  in
  let report =
    verify_posix_source ~path:"command-capture.sh" ~source
      [
        { (scenario "runtime multiline capture") with args = [ "alpha beta" ] };
      ]
  in
  Alcotest.(check int) "capture scenario" 1 (List.length report.results)

let test_all_scenarios_equivalent () =
  let scenarios = [ scenario "a"; scenario "b" ] in
  let execute scenario = Ok (observation scenario.Scenario.name) in
  let report =
    Differential.run ~scenarios ~original:execute ~migrated:execute
  in
  Alcotest.(check bool) "verified" true report.verified;
  Alcotest.(check (list string)) "coverage" [ "a"; "b" ] report.scenarios;
  Alcotest.(check int) "digest" 64 (String.length report.digest);
  Alcotest.(check int) "results" 2 (List.length report.results)

let test_difference_blocks_verification () =
  let scenarios = [ scenario "changed" ] in
  let report =
    Differential.run ~scenarios
      ~original:(fun _ -> Ok (observation "before"))
      ~migrated:(fun _ -> Ok (observation "after"))
  in
  Alcotest.(check bool) "not verified" false report.verified;
  match report.results with
  | [ Differential.Different comparison ] ->
      Alcotest.(check (list string))
        "stdout" [ "stdout" ]
        (List.map Observation.dimension comparison.differences)
  | _ -> Alcotest.fail "expected a differential mismatch"

let test_executor_failure_is_attributed () =
  let report =
    Differential.run
      ~scenarios:[ scenario "broken" ]
      ~original:(fun _ -> Error "oracle unavailable")
      ~migrated:(fun _ -> Alcotest.fail "migrated side must not run")
  in
  Alcotest.(check bool) "not verified" false report.verified;
  match report.results with
  | [ Differential.Failed { side = Original; message } ] ->
      Alcotest.(check string) "message" "oracle unavailable" message
  | _ -> Alcotest.fail "failure must retain its side"

let test_declared_expectation_is_enforced_before_comparison () =
  let declared =
    {
      (scenario "expected") with
      Scenario.expect =
        {
          exit_code = Some 0;
          stdout = Some "declared";
          stderr = Some "";
          files = [];
        };
    }
  in
  let migrated_calls = ref 0 in
  let report =
    Differential.run ~scenarios:[ declared ]
      ~original:(fun _ -> Ok (observation "unexpected"))
      ~migrated:(fun _ ->
        incr migrated_calls;
        Ok (observation "unexpected"))
  in
  Alcotest.(check bool) "not verified" false report.verified;
  Alcotest.(check int) "migrated side not trusted" 0 !migrated_calls;
  match report.results with
  | [ Differential.Failed { side = Original; message } ] ->
      Alcotest.(check bool)
        "expectation diagnostic" true
        (Test_support.contains ~needle:"expect.stdout" message)
  | _ -> Alcotest.fail "expectation mismatch must fail the original oracle"

let test_expected_file_digest () =
  let digest = Sha256.hex "artifact" in
  let declared =
    {
      (scenario "file") with
      Scenario.expect =
        {
          exit_code = None;
          stdout = None;
          stderr = None;
          files = [ { path = "out.txt"; sha256 = digest } ];
        };
    }
  in
  let observed =
    {
      (observation "") with
      Observation.files =
        [ { path = "out.txt"; before = None; after = Some digest } ];
    }
  in
  let report =
    Differential.run ~scenarios:[ declared ]
      ~original:(fun _ -> Ok observed)
      ~migrated:(fun _ -> Ok observed)
  in
  Alcotest.(check bool) "expected file accepted" true report.verified

let test_promote_only_exhaustively_enumerated_nodes () =
  let exhaustive =
    Ir.node ~id:"exhaustive"
      ~guarantee:(Ir.Exhaustive { scenarios = [ "a"; "b" ] })
      (Ir.Exec (Ir.exec [ "echo" ]))
  in
  let residual =
    Ir.node ~id:"residual"
      ~guarantee:(Ir.Residual { reason = "dynamic" })
      (Ir.Opaque_capsule
         (Ir.opaque ~interpreter:"sh" ~source:"echo $x" ~reason:"dynamic"))
  in
  let root =
    Ir.node ~id:"root"
      ~guarantee:(Ir.Formal { basis = "sequence" })
      (Ir.Sequence [ exhaustive; residual ])
  in
  let plan =
    Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:root () ]
  in
  let report =
    Differential.run
      ~scenarios:[ scenario "a"; scenario "b" ]
      ~original:(fun scenario -> Ok (observation scenario.name))
      ~migrated:(fun scenario -> Ok (observation scenario.name))
  in
  let promoted = Differential.promote_plan report plan in
  let guarantees =
    Ir.fold_nodes
      (fun values node -> (node.Ir.id, node.guarantee) :: values)
      [] (List.hd promoted.tasks).body
  in
  begin match List.assoc "exhaustive" guarantees with
  | Ir.Differential { scenarios; observation_digest } ->
      Alcotest.(check (list string)) "same scenarios" [ "a"; "b" ] scenarios;
      Alcotest.(check string) "digest" report.digest observation_digest
  | _ -> Alcotest.fail "exhaustive node was not promoted"
  end;
  begin match List.assoc "residual" guarantees with
  | Ir.Residual _ -> ()
  | _ -> Alcotest.fail "residual uncertainty must not be erased"
  end

let () =
  Alcotest.run "Differential engine"
    [
      ( "scenario execution",
        [
          Alcotest.test_case "equivalent" `Quick test_all_scenarios_equivalent;
          Alcotest.test_case "different" `Quick
            test_difference_blocks_verification;
          Alcotest.test_case "executor failure" `Quick
            test_executor_failure_is_attributed;
          Alcotest.test_case "declared expectations" `Quick
            test_declared_expectation_is_enforced_before_comparison;
          Alcotest.test_case "expected file" `Quick test_expected_file_digest;
          Alcotest.test_case "promotion" `Quick
            test_promote_only_exhaustively_enumerated_nodes;
          Alcotest.test_case "official POSIX branch state" `Quick
            test_posix_branch_state_matches_official_shell;
          Alcotest.test_case "official POSIX safe unquoted expansion" `Quick
            test_posix_safe_unquoted_matches_official_shell;
          Alcotest.test_case "official POSIX command capture" `Quick
            test_posix_command_capture_matches_official_shell;
        ] );
    ]
