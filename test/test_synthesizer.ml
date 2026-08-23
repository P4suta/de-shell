open Deshell

let plan stdout =
  let node =
    Ir.node ~id:"candidate-root"
      ~guarantee:(Ir.Residual { reason = "unverified synthesized candidate" })
      (Ir.Exec (Ir.exec [ "printf"; stdout ]))
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:node () ]

let candidate_json candidate_plan =
  `Assoc
    [
      ("provider", `String "test-synthesizer/1");
      ("rationale", `String "replace a dynamic fragment");
      ("plan", Ir_codec.encode_yojson candidate_plan);
      ("future", `Bool true);
    ]

let report verified =
  Differential.
    {
      verified;
      scenarios = [ "default" ];
      results = [];
      digest = String.make 64 (if verified then 'a' else 'b');
    }

let test_candidate_requires_normal_verification () =
  let candidate =
    match Synthesizer.decode_candidate (candidate_json (plan "candidate")) with
    | Ok value -> value
    | Error errors -> Alcotest.fail (String.concat "; " errors)
  in
  begin match
    Synthesizer.validate ~verify:(fun _ -> Ok (report false)) candidate
  with
  | Ok _ -> Alcotest.fail "an unverified AI candidate was promoted"
  | Error message ->
      Alcotest.(check bool)
        "verification diagnostic" true
        (Test_support.contains ~needle:"differential" message)
  end;
  match Synthesizer.validate ~verify:(fun _ -> Ok (report true)) candidate with
  | Error message -> Alcotest.fail message
  | Ok accepted ->
      Alcotest.(check string)
        "provider retained" "test-synthesizer/1" accepted.candidate.provider;
      Alcotest.(check string)
        "evidence digest" (String.make 64 'a') accepted.report.digest

let test_malformed_candidate_is_rejected () =
  match
    Synthesizer.decode_candidate
      (`Assoc
         [
           ("provider", `String "provider");
           ("rationale", `String "reason");
           ("plan", `Assoc []);
         ])
  with
  | Ok _ -> Alcotest.fail "malformed synthesized IR was accepted"
  | Error errors -> Alcotest.(check bool) "errors" true (errors <> [])

let () =
  Alcotest.run "Optional synthesizer boundary"
    [
      ( "candidate admission",
        [
          Alcotest.test_case "differential gate" `Quick
            test_candidate_requires_normal_verification;
          Alcotest.test_case "malformed plan" `Quick
            test_malformed_candidate_is_rejected;
        ] );
    ]
