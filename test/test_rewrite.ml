open Deshell

let test_simple_backticks () =
  let result = Rewrite.equivalent ~path:"simple.sh" "echo `printf hi`\n" in
  Alcotest.(check string) "rewritten" "echo $(printf hi)\n" result.output;
  match result.edits with
  | [ edit ] ->
      Alcotest.(check string) "rule" "posix.backticks.simple" edit.rule;
      Alcotest.(check int) "source start" 5 edit.original.start_byte;
      Alcotest.(check int) "source end" 16 edit.original.end_byte
  | _ -> Alcotest.fail "expected exactly one source-mapped edit"

let test_single_quotes_are_not_rewritten () =
  let source = "printf '%s' '`date`'\n" in
  let result = Rewrite.equivalent ~path:"quoted.sh" source in
  Alcotest.(check string) "unchanged" source result.output;
  Alcotest.(check int) "no edits" 0 (List.length result.edits)

let test_unsafe_backticks_are_not_rewritten () =
  let source = "echo `printf \\`nested\\``\n" in
  let result = Rewrite.equivalent ~path:"nested.sh" source in
  Alcotest.(check string) "unchanged" source result.output

let test_idempotence () =
  let first = Rewrite.equivalent ~path:"twice.sh" "echo `printf hi`\n" in
  let second = Rewrite.equivalent ~path:"twice.sh" first.output in
  Alcotest.(check string) "fixed point" first.output second.output;
  Alcotest.(check int) "no second edit" 0 (List.length second.edits)

let () =
  Alcotest.run "Equivalent rewrite"
    [
      ( "backticks",
        [
          Alcotest.test_case "simple" `Quick test_simple_backticks;
          Alcotest.test_case "single quoted negative" `Quick
            test_single_quotes_are_not_rewritten;
          Alcotest.test_case "unsafe negative" `Quick
            test_unsafe_backticks_are_not_rewritten;
          Alcotest.test_case "idempotent" `Quick test_idempotence;
        ] );
    ]
