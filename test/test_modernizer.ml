open Deshell

let test_secure_strict_mode () =
  let source = "#!/bin/sh\necho hello\n" in
  let result =
    Modernizer.propose ~path:"build.sh" ~profiles:[ Modernizer.Secure ] source
  in
  Alcotest.(check string)
    "inserted after shebang" "#!/bin/sh\nset -eu\necho hello\n" result.output;
  Alcotest.(check int) "one edit" 1 (List.length result.edits);
  Alcotest.(check string)
    "rule" "secure.strict-mode" (List.hd result.edits).Rewrite.rule

let test_already_strict_negative () =
  let source = "#!/bin/sh\nset -eu\necho hello\n" in
  let result =
    Modernizer.propose ~path:"build.sh" ~profiles:[ Modernizer.Secure ] source
  in
  Alcotest.(check string) "unchanged" source result.output;
  Alcotest.(check int) "no edit" 0 (List.length result.edits)

let test_profile_isolation () =
  let source = "#!/bin/sh\necho hello\n" in
  let result =
    Modernizer.propose ~path:"build.sh" ~profiles:[ Modernizer.Portable ] source
  in
  Alcotest.(check string) "secure rule not selected" source result.output

let test_idempotence () =
  let first =
    Modernizer.propose ~path:"build.sh" ~profiles:[ Modernizer.Secure ]
      "#!/bin/sh\necho hello\n"
  in
  let second =
    Modernizer.propose ~path:"build.sh" ~profiles:[ Modernizer.Secure ]
      first.output
  in
  Alcotest.(check string) "fixed point" first.output second.output;
  Alcotest.(check int) "no second edit" 0 (List.length second.edits)

let test_dangerous_pipe_is_suggestion_only () =
  let source = "curl https://example.invalid/install | sh\n" in
  let result =
    Modernizer.propose ~path:"install.sh" ~profiles:[ Modernizer.Secure ] source
  in
  Alcotest.(check bool)
    "finding" true
    (List.exists
       (fun (finding : Modernizer.finding) ->
         finding.rule = "secure.remote-code-pipe")
       result.findings);
  Alcotest.(check bool)
    "not silently rewritten" true
    (Test_support.contains ~needle:"curl https://" result.output)

let () =
  Alcotest.run "Modernization proposals"
    [
      ( "profiles",
        [
          Alcotest.test_case "secure strict mode" `Quick test_secure_strict_mode;
          Alcotest.test_case "already strict" `Quick
            test_already_strict_negative;
          Alcotest.test_case "profile isolation" `Quick test_profile_isolation;
          Alcotest.test_case "idempotence" `Quick test_idempotence;
          Alcotest.test_case "remote pipe suggestion" `Quick
            test_dangerous_pipe_is_suggestion_only;
        ] );
    ]
