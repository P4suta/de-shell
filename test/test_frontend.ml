open Deshell

let expect_exec node expected =
  match node.Ir.operation with
  | Ir.Exec command ->
      Alcotest.(check (list string)) "argv" expected command.argv
  | _ -> Alcotest.fail "expected Exec"

let test_literal_command () =
  let result =
    Posix_frontend.lower ~path:"hello.sh" "printf '%s\\n' 'hello world'\n"
  in
  expect_exec result.root [ "printf"; "%s\\n"; "hello world" ];
  match result.root.guarantee with
  | Ir.Formal _ -> ()
  | _ ->
      Alcotest.fail
        "literal POSIX command should receive a formal lowering guarantee"

let test_pipeline () =
  let result =
    Posix_frontend.lower ~path:"pipe.sh" "printf abc | tr a-z A-Z\n"
  in
  match result.root.operation with
  | Ir.Pipeline [ left; right ] ->
      expect_exec left [ "printf"; "abc" ];
      expect_exec right [ "tr"; "a-z"; "A-Z" ]
  | _ -> Alcotest.fail "expected a two-stage Pipeline"

let test_sequence_and_comments () =
  let source = "#!/bin/sh\n# explain\nprintf one\nprintf two\n" in
  let result = Posix_frontend.lower ~path:"sequence.sh" source in
  match result.root.operation with
  | Ir.Sequence [ first; second ] ->
      expect_exec first [ "printf"; "one" ];
      expect_exec second [ "printf"; "two" ]
  | _ -> Alcotest.fail "comments and shebang should not become effects"

let test_dynamic_command_becomes_capsule () =
  let source = "printf '%s\\n' \"$MESSAGE\"\n" in
  let result = Posix_frontend.lower ~path:"dynamic.sh" source in
  match (result.root.operation, result.root.guarantee) with
  | Ir.Opaque_capsule capsule, Ir.Residual evidence ->
      Alcotest.(check string) "source preserved" source capsule.source;
      Alcotest.(check bool)
        "reason explains dynamic syntax" true
        (Test_support.contains ~needle:"dynamic" evidence.reason)
  | _ ->
      Alcotest.fail
        "dynamic expansion must remain executable as a residual capsule"

let test_unterminated_quote_is_residual () =
  let source = "printf 'unterminated\n" in
  let result = Posix_frontend.lower ~path:"broken.sh" source in
  match result.root.operation with
  | Ir.Opaque_capsule capsule ->
      Alcotest.(check string) "lossless source" source capsule.source
  | _ -> Alcotest.fail "invalid syntax must not be guessed"

let test_double_quote_preserves_non_special_backslash () =
  let result =
    Posix_frontend.lower ~path:"quoted.sh" "printf '%s\\n' \"a\\qb\"\n"
  in
  expect_exec result.root [ "printf"; "%s\\n"; "a\\qb" ]

let test_shell_state_builtin_is_residual () =
  let source = "cd /tmp\n" in
  let result = Posix_frontend.lower ~path:"state.sh" source in
  match result.root.operation with
  | Ir.Opaque_capsule capsule ->
      Alcotest.(check string) "lossless source" source capsule.source
  | _ -> Alcotest.fail "stateful shell builtins must not become host Exec nodes"

let check_span label ~start_line ~start_column ~end_line ~end_column ~start_byte
    ~end_byte node =
  match node.Ir.source with
  | None -> Alcotest.fail (label ^ " has no source span")
  | Some span ->
      Alcotest.(check int) (label ^ " start line") start_line span.start_line;
      Alcotest.(check int)
        (label ^ " start column") start_column span.start_column;
      Alcotest.(check int) (label ^ " end line") end_line span.end_line;
      Alcotest.(check int) (label ^ " end column") end_column span.end_column;
      Alcotest.(check int) (label ^ " start byte") start_byte span.start_byte;
      Alcotest.(check int) (label ^ " end byte") end_byte span.end_byte

let test_precise_source_maps () =
  let source =
    "#!/bin/sh\n# setup\n  MODE=test printf one | tr o O\nprintf two\n"
  in
  let result = Posix_frontend.lower ~path:"mapped.sh" source in
  match result.root.operation with
  | Ir.Sequence [ pipeline; second ] ->
      begin match pipeline.operation with
      | Ir.Pipeline [ first; transform ] ->
          check_span "first" ~start_line:3 ~start_column:2 ~end_line:3
            ~end_column:22 ~start_byte:20 ~end_byte:40 first;
          check_span "transform" ~start_line:3 ~start_column:25 ~end_line:3
            ~end_column:31 ~start_byte:43 ~end_byte:49 transform;
          check_span "pipeline" ~start_line:3 ~start_column:2 ~end_line:3
            ~end_column:31 ~start_byte:20 ~end_byte:49 pipeline
      | _ -> Alcotest.fail "expected pipeline"
      end;
      check_span "second" ~start_line:4 ~start_column:0 ~end_line:4
        ~end_column:10 ~start_byte:50 ~end_byte:60 second;
      check_span "sequence" ~start_line:3 ~start_column:2 ~end_line:4
        ~end_column:10 ~start_byte:20 ~end_byte:60 result.root
  | _ -> Alcotest.fail "expected sequence"

let test_and_condition () =
  let source = "test -f ready && printf ready" in
  let result = Posix_frontend.lower ~path:"and.sh" source in
  match result.root.operation with
  | Ir.Condition { predicate; if_true; if_false = None } ->
      expect_exec predicate [ "test"; "-f"; "ready" ];
      expect_exec if_true [ "printf"; "ready" ];
      check_span "condition" ~start_line:1 ~start_column:0 ~end_line:1
        ~end_column:(String.length source) ~start_byte:0
        ~end_byte:(String.length source) result.root
  | _ -> Alcotest.fail "static && must lower to Condition"

let test_if_then_else () =
  let source = "if test -f ready; then printf yes; else printf no; fi" in
  let result = Posix_frontend.lower ~path:"if.sh" source in
  match result.root.operation with
  | Ir.Condition { predicate; if_true; if_false = Some if_false } ->
      expect_exec predicate [ "test"; "-f"; "ready" ];
      expect_exec if_true [ "printf"; "yes" ];
      expect_exec if_false [ "printf"; "no" ];
      begin match result.root.guarantee with
      | Ir.Formal { basis } ->
          Alcotest.(check bool)
            "basis" true
            (Test_support.contains ~needle:"condition" basis)
      | _ -> Alcotest.fail "static condition should be formally lowered"
      end
  | _ -> Alcotest.fail "if/then/else must lower to Condition"

let test_if_without_else_has_successful_false_branch () =
  let result = Posix_frontend.lower ~path:"if.sh" "if false; then true; fi" in
  match result.root.operation with
  | Ir.Condition { if_false = Some branch; _ } ->
      begin match branch.operation with
      | Ir.Sequence [] -> ()
      | _ -> Alcotest.fail "false branch must be a no-op"
      end
  | _ ->
      Alcotest.fail
        "POSIX if without else must return success when no condition matched"

let test_static_foreach () =
  let source = "for item in alpha beta; do printf '%s' \"$item\"; done" in
  let result = Posix_frontend.lower ~path:"for.sh" source in
  match result.root.operation with
  | Ir.For_each { variable; items; body } ->
      Alcotest.(check string) "variable" "item" variable;
      Alcotest.(check (list string)) "items" [ "alpha"; "beta" ] items;
      expect_exec body [ "printf"; "%s"; "${item}" ];
      check_span "foreach" ~start_line:1 ~start_column:0 ~end_line:1
        ~end_column:(String.length source) ~start_byte:0
        ~end_byte:(String.length source) result.root
  | _ -> Alcotest.fail "static for loop must lower to For_each"

let test_dynamic_foreach_is_residual () =
  let source = "for item in $ITEMS; do printf '%s' \"$item\"; done" in
  let result = Posix_frontend.lower ~path:"dynamic-for.sh" source in
  match (result.root.operation, result.root.guarantee) with
  | Ir.Opaque_capsule capsule, Ir.Residual _ ->
      Alcotest.(check string) "source retained" source capsule.source
  | _ -> Alcotest.fail "dynamic iteration space must remain residual"

let test_strict_script_dataflow () =
  let source =
    "#!/bin/sh\n\
     set -eu\n\
     destination=${1:-target/oracle}\n\
     archive=\"$destination/ncurses.tar.gz\"\n\
     mkdir -p \"$destination\"\n\
     printf '%s\\n' \"$archive\"\n"
  in
  let result = Posix_frontend.lower ~path:"fetch.sh" source in
  Alcotest.(check (list string))
    "no diagnostics" []
    (List.map
       (fun diagnostic -> diagnostic.Posix_frontend.message)
       result.diagnostics);
  match result.root.operation with
  | Ir.Condition { predicate; if_true; if_false = None } ->
      expect_exec predicate [ "mkdir"; "-p"; "${1:-target/oracle}" ];
      expect_exec if_true
        [ "printf"; "%s\\n"; "${1:-target/oracle}/ncurses.tar.gz" ];
      begin match (predicate.source, if_true.source) with
      | Some predicate_span, Some branch_span ->
          Alcotest.(check int)
            "predicate source line" 5 predicate_span.start_line;
          Alcotest.(check int) "branch source line" 6 branch_span.start_line
      | _ -> Alcotest.fail "strict lowered nodes must retain source maps"
      end
  | _ ->
      Alcotest.fail
        "set -e script must lower immutable assignments to a fail-fast \
         condition"

let test_strict_script_command_substitution_is_residual () =
  let source =
    "#!/bin/sh\nset -eu\nvalue=$(date)\nprintf '%s\\n' \"$value\"\n"
  in
  let result = Posix_frontend.lower ~path:"dynamic-assignment.sh" source in
  match (result.root.operation, result.root.guarantee) with
  | Ir.Opaque_capsule capsule, Ir.Residual _ ->
      Alcotest.(check string) "lossless source" source capsule.source
  | _ -> Alcotest.fail "command substitution must remain a residual capsule"

let test_strict_multiline_if_from_real_automation () =
  let source =
    "#!/bin/sh\n\
     set -eu\n\
     destination=${1:-target/oracle}\n\
     archive=\"$destination/ncurses.tar.gz\"\n\
     if [ -f \"$archive\" ] &&\n\
     verify \"$archive\"\n\
     then\n\
     printf '%s\\n' \"reuse $archive\"\n\
     else\n\
     fetch --output \"$archive\"\n\
     fi\n\
     unpack \"$archive\" \"$destination\"\n"
  in
  let result = Posix_frontend.lower ~path:"fetch-real.sh" source in
  Alcotest.(check bool)
    "real strict automation is non-residual" false
    (Posix_frontend.has_residual result.root);
  begin match
    Ir.validate_plan
      (Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:result.root () ])
  with
  | Ok () -> ()
  | Error errors ->
      Alcotest.fail
        ("strict automation produced invalid IR: " ^ String.concat "; " errors)
  end;
  match result.root.operation with
  | Ir.Condition
      {
        predicate =
          {
            operation =
              Ir.Condition
                {
                  predicate = { operation = Ir.Condition _; _ };
                  if_false = Some _;
                  _;
                };
            _;
          };
        if_true = { operation = Ir.Exec _; _ };
        if_false = None;
      } ->
      ()
  | _ ->
      Alcotest.fail
        "strict script must preserve top-level fail-fast and multiline if \
         control"

let test_strict_fail_fast_node_ids_are_unique () =
  let result =
    Posix_frontend.lower ~path:"strict-sequence.sh"
      "#!/bin/sh\nset -eu\nprintf one\nprintf two\nprintf three\nprintf four\n"
  in
  match
    Ir.validate_plan
      (Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:result.root () ])
  with
  | Ok () -> ()
  | Error errors ->
      Alcotest.fail
        ("strict sequence produced invalid IR: " ^ String.concat "; " errors)

let test_strict_fail_fast_execution () =
  let result =
    Posix_frontend.lower ~path:"strict-failure.sh"
      "#!/bin/sh\nset -eu\nfail-now\nmust-not-run\n"
  in
  let calls = ref [] in
  let backend : Runner.backend =
    {
      execute =
        (fun request ->
          calls := request.argv :: !calls;
          Ok
            Runner.
              {
                exit_code = (if request.argv = [ "fail-now" ] then 9 else 0);
                stdout = "";
                stderr = "";
              });
      read_file = (fun _ -> Error "unused");
      write_file = (fun ~path:_ ~contents:_ ~append:_ -> Error "unused");
      remove_file = (fun _ -> Error "unused");
      network_request = (fun ~method_:_ ~uri:_ -> Error "unused");
    }
  in
  let plan =
    Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:result.root () ]
  in
  begin match Runner.run_plan ~backend ~policy:Runner.default_policy plan with
  | Error message -> Alcotest.fail message
  | Ok observation ->
      Alcotest.(check int) "first failure returned" 9 observation.exit_code
  end;
  Alcotest.(check (list (list string)))
    "later command skipped" [ [ "fail-now" ] ] (List.rev !calls)

let test_strict_unsafe_state_stays_residual () =
  let cases =
    [
      ("unquoted expansion", "#!/bin/sh\nset -eu\nvalue=ok\nprintf %s $value\n");
      ( "late assignment",
        "#!/bin/sh\n\
         set -eu\n\
         value=first\n\
         printf '%s' \"$value\"\n\
         printf first\n\
         value=second\n\
         printf '%s' \"$value\"\n" );
      ( "assignment after prior reference",
        "#!/bin/sh\n\
         set -eu\n\
         printf '%s' \"${late-default}\"\n\
         late=second\n\
         printf '%s' \"$late\"\n" );
      ( "conditional late assignment",
        "#!/bin/sh\n\
         set -eu\n\
         if test -f ready\n\
         then\n\
         mode=release\n\
         fi\n\
         tool.exe build \"$mode\"\n" );
      ( "mutating parameter operator",
        "#!/bin/sh\nset -eu\nprintf '%s' \"${MODE:=default}\"\n" );
      ("unsupported option", "#!/bin/sh\nset -eux\nprintf traced\n");
      ( "dynamic top-level cwd",
        "#!/bin/sh\nset -eu\ncd \"$TARGET\"\ntool.exe build\n" );
      ( "multiple script cwd declarations",
        "#!/bin/sh\n\
         set -eu\n\
         cd \"$(dirname \"$0\")\"\n\
         cd -- \"$(dirname -- \"$0\")\"\n\
         tool.exe build\n" );
    ]
  in
  List.iter
    (fun (label, source) ->
      let result = Posix_frontend.lower ~path:(label ^ ".sh") source in
      Alcotest.(check bool)
        (label ^ " remains residual")
        true
        (Posix_frontend.has_residual result.root);
      match result.root.operation with
      | Ir.Opaque_capsule capsule ->
          Alcotest.(check string) (label ^ " source") source capsule.source
      | _ -> Alcotest.fail (label ^ " was partially lowered unsafely"))
    cases

let test_bracket_command_and_glob_boundary () =
  let condition =
    Posix_frontend.lower ~path:"bracket.sh" "[ -f ready ] && printf ready\n"
  in
  begin match condition.root.operation with
  | Ir.Condition { predicate; _ } ->
      expect_exec predicate [ "["; "-f"; "ready"; "]" ]
  | _ -> Alcotest.fail "standalone [ command must lower as a condition"
  end;
  let glob = Posix_frontend.lower ~path:"glob.sh" "printf '%s\\n' [ab]\n" in
  Alcotest.(check bool)
    "bracket glob remains dynamic" true
    (Posix_frontend.has_residual glob.root)

let test_find_exec_placeholder_is_literal () =
  let result =
    Posix_frontend.lower ~path:"find.sh"
      "find output -type f -exec tool.exe --check {} +\n"
  in
  expect_exec result.root
    [
      "find"; "output"; "-type"; "f"; "-exec"; "tool.exe"; "--check"; "{}"; "+";
    ];
  [ "printf '%s' {a,b}\n"; "printf '%s' \"${VALUE}\"\n" ]
  |> List.iter (fun source ->
      let dynamic = Posix_frontend.lower ~path:"brace.sh" source in
      Alcotest.(check bool)
        "actual brace expansion stays residual" true
        (Posix_frontend.has_residual dynamic.root))

let test_single_quoted_template_is_literal () =
  let result =
    Posix_frontend.lower ~path:"literal-template.sh"
      "printf '%s\\n' '${HOME}'\n"
  in
  expect_exec result.root [ "printf"; "%s\\n"; "$${HOME}" ]

let test_strict_literal_dollars_survive_dataflow () =
  let result =
    Posix_frontend.lower ~path:"literal-dataflow.sh"
      "#!/bin/sh\n\
       set -eu\n\
       literal='${HOME}'\n\
       printf '%s:%s\\n' \"$literal\" \"\\${HOME}\"\n"
  in
  expect_exec result.root [ "printf"; "%s:%s\\n"; "$${HOME}"; "$${HOME}" ]

let test_strict_or_short_circuit_execution () =
  let lowered =
    Posix_frontend.lower ~path:"or.sh"
      "#!/bin/sh\nset -eu\nprobe || fallback\nafter\n"
  in
  Alcotest.(check bool)
    "static OR is non-residual" false
    (Posix_frontend.has_residual lowered.root);
  let run probe_status =
    let calls = ref [] in
    let backend : Runner.backend =
      {
        execute =
          (fun request ->
            calls := request.argv :: !calls;
            Ok
              Runner.
                {
                  exit_code =
                    (if request.argv = [ "probe" ] then probe_status else 0);
                  stdout = "";
                  stderr = "";
                });
        read_file = (fun _ -> Error "unused");
        write_file = (fun ~path:_ ~contents:_ ~append:_ -> Error "unused");
        remove_file = (fun _ -> Error "unused");
        network_request = (fun ~method_:_ ~uri:_ -> Error "unused");
      }
    in
    let plan =
      Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:lowered.root () ]
    in
    begin match Runner.run_plan ~backend ~policy:Runner.default_policy plan with
    | Error message -> Alcotest.fail message
    | Ok observation ->
        Alcotest.(check int) "final status" 0 observation.exit_code
    end;
    List.rev !calls
  in
  Alcotest.(check (list (list string)))
    "failure runs fallback"
    [ [ "probe" ]; [ "fallback" ]; [ "after" ] ]
    (run 7);
  Alcotest.(check (list (list string)))
    "success skips fallback"
    [ [ "probe" ]; [ "after" ] ]
    (run 0)

let test_strict_mixed_and_or_stays_residual () =
  let result =
    Posix_frontend.lower ~path:"mixed-boolean.sh"
      "#!/bin/sh\nset -eu\nfirst && second || third\n"
  in
  Alcotest.(check bool)
    "mixed associativity is not guessed" true
    (Posix_frontend.has_residual result.root)

let test_strict_pipefail_without_pipeline_is_static () =
  [
    ( "combined",
      "#!/usr/bin/env bash\nset -euo pipefail\nprintf first\nprintf second\n" );
    ( "shebang options",
      "#!/bin/bash -eu\nset -o pipefail\nprintf first\nprintf second\n" );
  ]
  |> List.iter (fun (label, source) ->
      let result = Posix_frontend.lower ~path:(label ^ ".sh") source in
      Alcotest.(check bool)
        (label ^ " is non-residual")
        false
        (Posix_frontend.has_residual result.root);
      match result.root.operation with
      | Ir.Condition _ -> ()
      | _ ->
          Alcotest.fail
            (label ^ " must preserve errexit as a fail-fast condition"))

let test_strict_header_assignment_comments () =
  let source =
    "#!/bin/sh\n\
     set -eu # fail closed\n\
     TOOL='tool#stable' # executable selected by the build\n\
     MODE=release # immutable mode\n\
     \"$TOOL\" build \"$MODE\"\n"
  in
  let result = Posix_frontend.lower ~path:"commented-header.sh" source in
  Alcotest.(check bool)
    "commented constants are non-residual" false
    (Posix_frontend.has_residual result.root);
  expect_exec result.root [ "tool#stable"; "build"; "release" ]

let test_strict_late_command_environment_is_not_mutable_state () =
  let source =
    "#!/bin/sh\nset -eu\nprintf first\nLC_ALL=C tool.exe build\nprintf last\n"
  in
  let result = Posix_frontend.lower ~path:"command-environment.sh" source in
  Alcotest.(check bool)
    "command-local environment is non-residual" false
    (Posix_frontend.has_residual result.root);
  let command =
    Ir.fold_nodes
      (fun found node ->
        match (found, node.Ir.operation) with
        | Some _, _ -> found
        | None, Ir.Exec command when command.argv = [ "tool.exe"; "build" ] ->
            Some command
        | None, _ -> None)
      None result.root
    |> Option.get
  in
  Alcotest.(check (list (pair string string)))
    "command environment"
    [ ("LC_ALL", "C") ]
    command.environment

let test_strict_late_immutable_assignment () =
  let source =
    "#!/bin/sh\n\
     set -eu\n\
     printf preparing\n\
     MODE=release # first declaration after an external command\n\
     tool.exe build \"$MODE\"\n"
  in
  let result = Posix_frontend.lower ~path:"late-constant.sh" source in
  begin if Posix_frontend.has_residual result.root then
    match result.root.guarantee with
    | Ir.Residual evidence ->
        Alcotest.fail ("new late constant is non-residual: " ^ evidence.reason)
    | _ -> Alcotest.fail "new late constant contains a nested residual node"
  end;
  let command =
    Ir.fold_nodes
      (fun found node ->
        match (found, node.Ir.operation) with
        | Some _, _ -> found
        | None, Ir.Exec command
          when command.argv = [ "tool.exe"; "build"; "release" ] ->
            Some command
        | None, _ -> None)
      None result.root
  in
  Alcotest.(check bool)
    "late binding is applied only to subsequent argv" true
    (Option.is_some command)

let test_strict_top_level_assignment_after_closed_control_flow () =
  let source =
    "#!/bin/sh\n\
     set -eu\n\
     if probe\n\
     then\n\
     prepare-one\n\
     else\n\
     prepare-two\n\
     fi\n\
     MODE=release\n\
     tool.exe build \"$MODE\"\n"
  in
  let result = Posix_frontend.lower ~path:"post-control-constant.sh" source in
  begin if Posix_frontend.has_residual result.root then
    match result.root.guarantee with
    | Ir.Residual evidence ->
        Alcotest.fail
          ("top-level post-control constant is safe: " ^ evidence.reason)
    | _ -> Alcotest.fail "post-control constant contains a nested residual"
  end;
  let found =
    Ir.fold_nodes
      (fun found node ->
        found
        ||
        match node.Ir.operation with
        | Ir.Exec command -> command.argv = [ "tool.exe"; "build"; "release" ]
        | _ -> false)
      false result.root
  in
  Alcotest.(check bool) "subsequent argv uses the constant" true found

let test_pipefail_pipeline_stays_residual () =
  let source =
    "#!/usr/bin/env bash\nset -euo pipefail\nproduce | consume\nprintf after\n"
  in
  let result = Posix_frontend.lower ~path:"pipefail.sh" source in
  match (result.root.operation, result.root.guarantee) with
  | Ir.Opaque_capsule capsule, Ir.Residual evidence ->
      Alcotest.(check string) "lossless source" source capsule.source;
      Alcotest.(check bool)
        "precise reason" true
        (Test_support.contains ~needle:"pipefail" evidence.reason)
  | _ ->
      Alcotest.fail
        "pipefail pipeline must remain residual until its rightmost-nonzero \
         status is represented"

let test_strict_packaging_cwd_heredoc_and_subshell () =
  let source =
    "#!/usr/bin/env bash\n\
     set -euo pipefail\n\
     cd \"$(dirname \"$0\")\"\n\
     DIST=dist\n\
     TOOL=tool.exe\n\
     \"$TOOL\" build \"$DIST\"\n\
     cat > \"$DIST/README.txt\" <<'EOF'\n\
     literal ${HOME}\n\
     EOF\n\
     ( cd \"$DIST\" && archive.exe bundle.zip . )\n"
  in
  let result = Posix_frontend.lower ~path:"packaging/package.sh" source in
  let repeated = Posix_frontend.lower ~path:"packaging/package.sh" source in
  Alcotest.(check bool)
    "strict lowering is deterministic" true
    (result.root = repeated.root && result.diagnostics = repeated.diagnostics);
  let diagnostic =
    result.diagnostics
    |> List.map (fun value -> value.Posix_frontend.message)
    |> String.concat "; "
  in
  Alcotest.(check bool)
    ("packaging subset is non-residual: " ^ diagnostic)
    false
    (Posix_frontend.has_residual result.root);
  let commands, writes =
    Ir.fold_nodes
      (fun (commands, writes) node ->
        match node.Ir.operation with
        | Ir.Exec command -> (command :: commands, writes)
        | Ir.File_write write -> (commands, write :: writes)
        | _ -> (commands, writes))
      ([], []) result.root
  in
  let find_command executable =
    List.find
      (fun command ->
        match command.Ir.argv with
        | value :: _ -> value = executable
        | [] -> false)
      commands
  in
  let build = find_command "tool.exe" in
  Alcotest.(check (option string))
    "script directory cwd" (Some "packaging") build.working_directory;
  let archive = find_command "archive.exe" in
  Alcotest.(check (option string))
    "subshell cwd" (Some "packaging/dist") archive.working_directory;
  match writes with
  | [ write ] ->
      Alcotest.(check string)
        "heredoc path" "packaging/dist/README.txt" write.path;
      Alcotest.(check string)
        "quoted heredoc is literal" "literal $${HOME}\n" write.contents
  | _ -> Alcotest.fail "packaging subset must contain one typed file write"

let file_writes root =
  Ir.fold_nodes
    (fun writes node ->
      match node.Ir.operation with
      | Ir.File_write write -> (node, write) :: writes
      | _ -> writes)
    [] root
  |> List.rev

let test_static_heredoc_quoting_modes () =
  [
    ( "double quoted delimiter",
      "cat > artifact.txt <<\"END\"\n\
       MODE=value\n\
       literal $HOME and `date` and \\ data\n\
       END\n",
      "MODE=value\nliteral $$HOME and `date` and \\ data\n" );
    ( "expansion-free unquoted delimiter",
      "cat > artifact.txt <<END\nplain text and \"quotes\"\nEND\n",
      "plain text and \"quotes\"\n" );
  ]
  |> List.iter (fun (label, heredoc, expected_contents) ->
      let source = "#!/bin/sh\nset -eu\n" ^ heredoc in
      let result = Posix_frontend.lower ~path:"heredoc.sh" source in
      let diagnostic =
        result.diagnostics
        |> List.map (fun value -> value.Posix_frontend.message)
        |> String.concat "; "
      in
      Alcotest.(check bool)
        (label ^ " is non-residual: " ^ diagnostic)
        false
        (Posix_frontend.has_residual result.root);
      match file_writes result.root with
      | [ (node, write) ] ->
          Alcotest.(check string)
            (label ^ " output path") "artifact.txt" write.path;
          Alcotest.(check string)
            (label ^ " contents") expected_contents write.contents;
          begin match node.source with
          | Some span ->
              Alcotest.(check int) (label ^ " start line") 3 span.start_line;
              let expected_end_line =
                if label = "double quoted delimiter" then 6 else 5
              in
              Alcotest.(check int)
                (label ^ " end line") expected_end_line span.end_line
          | None -> Alcotest.fail (label ^ " must preserve its source map")
          end
      | _ -> Alcotest.fail (label ^ " must lower to one FileWrite"))

let test_heredoc_expansion_boundaries_are_atomic () =
  [
    ( "unquoted parameter expansion",
      "cat > artifact.txt <<END\n$HOME\nEND\n",
      "expansion" );
    ( "unquoted command substitution",
      "cat > artifact.txt <<END\n`date`\nEND\n",
      "expansion" );
    ("tab stripping", "cat > artifact.txt <<-'END'\nvalue\nEND\n", "heredoc");
    ( "missing delimiter",
      "cat > artifact.txt <<'END'\nvalue\n",
      "missing delimiter" );
  ]
  |> List.iter (fun (label, heredoc, reason_fragment) ->
      let source = "#!/bin/sh\nset -eu\n" ^ heredoc in
      let result = Posix_frontend.lower ~path:"heredoc.sh" source in
      match (result.root.operation, result.root.guarantee) with
      | Ir.Opaque_capsule capsule, Ir.Residual evidence ->
          Alcotest.(check string) (label ^ " source") source capsule.source;
          Alcotest.(check bool)
            (label ^ " reason") true
            (Test_support.contains ~needle:reason_fragment evidence.reason)
      | _ -> Alcotest.fail (label ^ " must remain one lossless capsule"))

let () =
  Alcotest.run "POSIX frontend"
    [
      ( "lower",
        [
          Alcotest.test_case "literal command" `Quick test_literal_command;
          Alcotest.test_case "pipeline" `Quick test_pipeline;
          Alcotest.test_case "sequence" `Quick test_sequence_and_comments;
          Alcotest.test_case "dynamic residual" `Quick
            test_dynamic_command_becomes_capsule;
          Alcotest.test_case "invalid residual" `Quick
            test_unterminated_quote_is_residual;
          Alcotest.test_case "double quote backslash" `Quick
            test_double_quote_preserves_non_special_backslash;
          Alcotest.test_case "state builtin residual" `Quick
            test_shell_state_builtin_is_residual;
          Alcotest.test_case "precise source maps" `Quick
            test_precise_source_maps;
          Alcotest.test_case "and condition" `Quick test_and_condition;
          Alcotest.test_case "if then else" `Quick test_if_then_else;
          Alcotest.test_case "if false status" `Quick
            test_if_without_else_has_successful_false_branch;
          Alcotest.test_case "static foreach" `Quick test_static_foreach;
          Alcotest.test_case "dynamic foreach" `Quick
            test_dynamic_foreach_is_residual;
          Alcotest.test_case "strict script dataflow" `Quick
            test_strict_script_dataflow;
          Alcotest.test_case "strict dynamic assignment" `Quick
            test_strict_script_command_substitution_is_residual;
          Alcotest.test_case "strict multiline if" `Quick
            test_strict_multiline_if_from_real_automation;
          Alcotest.test_case "strict unique node IDs" `Quick
            test_strict_fail_fast_node_ids_are_unique;
          Alcotest.test_case "strict fail-fast execution" `Quick
            test_strict_fail_fast_execution;
          Alcotest.test_case "strict unsafe state" `Quick
            test_strict_unsafe_state_stays_residual;
          Alcotest.test_case "bracket command boundary" `Quick
            test_bracket_command_and_glob_boundary;
          Alcotest.test_case "find exec placeholder" `Quick
            test_find_exec_placeholder_is_literal;
          Alcotest.test_case "single-quoted template literal" `Quick
            test_single_quoted_template_is_literal;
          Alcotest.test_case "strict literal dollar dataflow" `Quick
            test_strict_literal_dollars_survive_dataflow;
          Alcotest.test_case "strict OR execution" `Quick
            test_strict_or_short_circuit_execution;
          Alcotest.test_case "strict mixed boolean residual" `Quick
            test_strict_mixed_and_or_stays_residual;
          Alcotest.test_case "strict pipefail without pipeline" `Quick
            test_strict_pipefail_without_pipeline_is_static;
          Alcotest.test_case "strict header comments" `Quick
            test_strict_header_assignment_comments;
          Alcotest.test_case "strict command environment" `Quick
            test_strict_late_command_environment_is_not_mutable_state;
          Alcotest.test_case "strict late immutable assignment" `Quick
            test_strict_late_immutable_assignment;
          Alcotest.test_case "strict post-control immutable assignment" `Quick
            test_strict_top_level_assignment_after_closed_control_flow;
          Alcotest.test_case "pipefail pipeline residual" `Quick
            test_pipefail_pipeline_stays_residual;
          Alcotest.test_case "strict packaging effects" `Quick
            test_strict_packaging_cwd_heredoc_and_subshell;
          Alcotest.test_case "static heredoc quoting modes" `Quick
            test_static_heredoc_quoting_modes;
          Alcotest.test_case "heredoc expansion boundaries" `Quick
            test_heredoc_expansion_boundaries_are_atomic;
        ] );
    ]
