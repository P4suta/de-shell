open Deshell

let formal id =
  Ir.node ~id
    ~guarantee:(Ir.Formal { basis = "test" })
    (Ir.Exec (Ir.exec [ "emit"; id ]))

let residual id =
  let capsule =
    Ir.opaque ~interpreter:"sh" ~source:"echo $x" ~reason:"dynamic"
  in
  Ir.node ~id
    ~guarantee:(Ir.Residual { reason = "dynamic" })
    (Ir.Opaque_capsule capsule)

let test_guarantee_audit () =
  let body =
    Ir.node ~id:"root"
      ~guarantee:(Ir.Formal { basis = "sequence" })
      (Ir.Sequence [ formal "one"; residual "two" ])
  in
  let plan = Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ] in
  match Verifier.audit plan with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok report ->
      Alcotest.(check int) "formal" 2 report.formal;
      Alcotest.(check int) "residual" 1 report.residual;
      Alcotest.(check (list string))
        "reason" [ "two: dynamic" ] report.residual_reasons

let observation ?(exit_code = 0) ?(stdout = "out") ?(stderr = "") trace =
  Runner.{ exit_code; stdout; stderr; trace }

let test_equal_observations () =
  let value = observation [ Runner.Process ([ "echo" ], 0) ] in
  let comparison = Verifier.compare_observations value value in
  Alcotest.(check bool) "equivalent" true comparison.equivalent;
  Alcotest.(check int) "no differences" 0 (List.length comparison.differences);
  Alcotest.(check int) "digest" 64 (String.length comparison.observation_digest)

let test_difference_dimensions () =
  let expected = observation [ Runner.Process ([ "echo" ], 0) ] in
  let actual =
    observation ~exit_code:2 ~stdout:"changed" ~stderr:"warning"
      [ Runner.Process ([ "other" ], 2) ]
  in
  let comparison = Verifier.compare_observations expected actual in
  Alcotest.(check bool) "not equivalent" false comparison.equivalent;
  Alcotest.(check (list string))
    "dimensions"
    [ "exit_code"; "stdout"; "stderr"; "trace" ]
    (List.map Verifier.dimension comparison.differences)

let () =
  Alcotest.run "Verification core"
    [
      ( "coverage",
        [ Alcotest.test_case "guarantee audit" `Quick test_guarantee_audit ] );
      ( "differential",
        [
          Alcotest.test_case "equal" `Quick test_equal_observations;
          Alcotest.test_case "dimensions" `Quick test_difference_dimensions;
        ] );
    ]
