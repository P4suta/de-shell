open Deshell

let node id guarantee = Ir.node ~id ~guarantee (Ir.Exec (Ir.exec [ "true" ]))

let sequence_plan ~formal ~residual =
  let nodes =
    List.init formal (fun index ->
        node
          (Printf.sprintf "formal-%d" index)
          (Ir.Formal { basis = "curated-corpus" }))
    @ List.init residual (fun index ->
        node
          (Printf.sprintf "residual-%d" index)
          (Ir.Residual { reason = "explicit capsule fallback" }))
  in
  let body =
    match nodes with
    | [ node ] -> node
    | nodes ->
        Ir.node ~id:"root"
          ~guarantee:(Ir.Formal { basis = "corpus sequence" })
          (Ir.Sequence nodes)
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ]

let complete_matrix passed =
  Release_gate.required_matrix
  |> List.map (fun (operating_system, shell) ->
      Release_gate.{ operating_system; shell; passed })

let test_ready_release () =
  let corpus =
    [
      Release_gate.
        {
          name = "portable";
          plan = sequence_plan ~formal:20 ~residual:1;
          non_interactive = true;
          executable_with_residual = true;
        };
    ]
  in
  let report =
    Release_gate.evaluate ~corpus ~unexplained_differences:0
      ~expected_embedded:[ "make"; "github-actions" ]
      ~found_embedded:[ "github-actions"; "make" ]
      ~matrix:(complete_matrix true)
  in
  Alcotest.(check bool) "ready" true report.ready;
  Alcotest.(check bool)
    "semantic coverage" true
    (report.non_residual_coverage >= 0.95);
  Alcotest.(check int) "21 platform gates" 21 report.matrix_passed

let test_each_release_failure_is_explained () =
  let corpus =
    [
      Release_gate.
        {
          name = "failing";
          plan = sequence_plan ~formal:1 ~residual:2;
          non_interactive = true;
          executable_with_residual = false;
        };
    ]
  in
  let matrix =
    match complete_matrix true with
    | first :: rest -> { first with passed = false } :: rest
    | [] -> Alcotest.fail "required matrix must not be empty"
  in
  let report =
    Release_gate.evaluate ~corpus ~unexplained_differences:1
      ~expected_embedded:[ "make"; "github-actions" ]
      ~found_embedded:[ "make" ] ~matrix
  in
  Alcotest.(check bool) "not ready" false report.ready;
  List.iter
    (fun needle ->
      Alcotest.(check bool)
        needle true
        (List.exists (Test_support.contains ~needle) report.failures))
    [ "difference"; "95%"; "residual"; "inventory"; "matrix" ]

let () =
  Alcotest.run "Version 1.0 release gate"
    [
      ( "acceptance",
        [
          Alcotest.test_case "all criteria" `Quick test_ready_release;
          Alcotest.test_case "actionable failures" `Quick
            test_each_release_failure_is_explained;
        ] );
    ]
