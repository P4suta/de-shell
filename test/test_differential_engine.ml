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
        ] );
    ]
