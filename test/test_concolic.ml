open Deshell

let environment name scenario =
  List.assoc_opt name scenario.Concolic.scenario.Scenario.environment

let test_discovers_inputs_and_branch_values () =
  let source =
    {|#!/bin/sh
if [ "$MODE" = release ]; then
  printf '%s' "$1"
fi
printf '%s' "${TARGET:-dev}"
|}
  in
  let result = Concolic.suggest ~max_scenarios:12 ~source in
  Alcotest.(check (list string))
    "environment references" [ "MODE"; "TARGET" ] result.environment_variables;
  Alcotest.(check (list int)) "argument references" [ 1 ] result.arguments;
  Alcotest.(check bool)
    "branch literal" true
    (List.exists
       (fun (candidate : Concolic.candidate) ->
         environment "MODE" candidate = Some "release")
       result.candidates);
  Alcotest.(check bool)
    "default value" true
    (List.exists
       (fun (candidate : Concolic.candidate) ->
         environment "TARGET" candidate = Some "dev")
       result.candidates);
  Alcotest.(check bool)
    "argument populated" true
    (List.exists
       (fun (candidate : Concolic.candidate) ->
         candidate.scenario.args = [ "deshell-arg-1" ])
       result.candidates)

let test_ignores_comments_and_single_quotes () =
  let source = "# $COMMENT\nprintf '%s' '$LITERAL'\nprintf '%s' \"$REAL\"\n" in
  let result = Concolic.suggest ~max_scenarios:8 ~source in
  Alcotest.(check (list string))
    "only evaluated variable" [ "REAL" ] result.environment_variables

let test_secret_values_are_placeholders () =
  let result =
    Concolic.suggest ~max_scenarios:8
      ~source:"printf '%s' \"$API_TOKEN:$PASSWORD\"\n"
  in
  let values =
    result.candidates
    |> List.concat_map (fun (candidate : Concolic.candidate) ->
        candidate.scenario.environment)
    |> List.map snd
  in
  Alcotest.(check bool)
    "redacted token marker" true
    (List.mem "<secret:API_TOKEN>" values);
  Alcotest.(check bool)
    "redacted password marker" true
    (List.mem "<secret:PASSWORD>" values)

let test_bound_and_determinism () =
  let source = "$A $B $C $D $E $1 $2 $3\n" in
  let left = Concolic.suggest ~max_scenarios:4 ~source in
  let right = Concolic.suggest ~max_scenarios:4 ~source in
  Alcotest.(check int) "bounded" 4 (List.length left.candidates);
  Alcotest.(check bool) "deterministic" true (left = right)

let () =
  Alcotest.run "Concolic scenario exploration"
    [
      ( "inputs",
        [
          Alcotest.test_case "references/branches" `Quick
            test_discovers_inputs_and_branch_values;
          Alcotest.test_case "quoting/comments" `Quick
            test_ignores_comments_and_single_quotes;
          Alcotest.test_case "secret placeholders" `Quick
            test_secret_values_are_placeholders;
          Alcotest.test_case "bound/determinism" `Quick
            test_bound_and_determinism;
        ] );
    ]
