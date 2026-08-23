open Deshell

let node index operation =
  Ir.node
    ~id:(Printf.sprintf "node-%d" index)
    ~guarantee:(Ir.Formal { basis = "runner-test" })
    operation

let plan body = Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ]

let backend calls : Runner.backend =
  let execute (request : Runner.process_request) =
    calls := request.argv :: !calls;
    match request.argv with
    | [ "emit"; value ] ->
        Ok Runner.{ exit_code = 0; stdout = value; stderr = "" }
    | [ "upper" ] ->
        Ok
          Runner.
            {
              exit_code = 0;
              stdout = String.uppercase_ascii request.stdin;
              stderr = "";
            }
    | [ "fail"; code ] ->
        Ok
          Runner.
            { exit_code = int_of_string code; stdout = ""; stderr = "failed" }
    | _ -> Ok Runner.{ exit_code = 0; stdout = request.stdin; stderr = "" }
  in
  {
    execute;
    read_file = (fun path -> Ok ("read:" ^ path));
    write_file = (fun ~path:_ ~contents:_ ~append:_ -> Ok ());
    remove_file = (fun _ -> Ok ());
    network_request = (fun ~method_:_ ~uri:_ -> Ok "network");
  }

let run calls ?(policy = Runner.default_policy) body =
  Runner.run_plan ~backend:(backend calls) ~policy (plan body)

let get = function Ok value -> value | Error message -> Alcotest.fail message

let test_pipeline_stream () =
  let calls = ref [] in
  let body =
    node 3
      (Ir.Pipeline
         [
           node 1 (Ir.Exec (Ir.exec [ "emit"; "hello" ]));
           node 2 (Ir.Exec (Ir.exec [ "upper" ]));
         ])
  in
  let observation = run calls body |> get in
  Alcotest.(check string) "stdout" "HELLO" observation.stdout;
  Alcotest.(check int) "exit" 0 observation.exit_code;
  Alcotest.(check (list (list string)))
    "order"
    [ [ "emit"; "hello" ]; [ "upper" ] ]
    (List.rev !calls)

let test_sequence_continues_after_failure () =
  let calls = ref [] in
  let body =
    node 3
      (Ir.Sequence
         [
           node 1 (Ir.Exec (Ir.exec [ "fail"; "9" ]));
           node 2 (Ir.Exec (Ir.exec [ "emit"; "after" ]));
         ])
  in
  let observation = run calls body |> get in
  Alcotest.(check int) "last status" 0 observation.exit_code;
  Alcotest.(check string) "later command ran" "after" observation.stdout;
  Alcotest.(check string) "stderr retained" "failed" observation.stderr

let test_finalizer_runs_and_preserves_failure () =
  let calls = ref [] in
  let body =
    node 3
      (Ir.Try_finally
         {
           body = node 1 (Ir.Exec (Ir.exec [ "fail"; "7" ]));
           finalizer = node 2 (Ir.Exec (Ir.exec [ "emit"; "cleanup" ]));
         })
  in
  let observation = run calls body |> get in
  Alcotest.(check int) "body status" 7 observation.exit_code;
  Alcotest.(check string) "cleanup output" "cleanup" observation.stdout;
  Alcotest.(check (list (list string)))
    "cleanup called"
    [ [ "fail"; "7" ]; [ "emit"; "cleanup" ] ]
    (List.rev !calls)

let test_residual_policy () =
  let calls = ref [] in
  let capsule =
    Ir.opaque ~interpreter:"sh" ~source:"echo capsule" ~reason:"test"
  in
  let body =
    Ir.node ~id:"capsule"
      ~guarantee:(Ir.Residual { reason = "test" })
      (Ir.Opaque_capsule capsule)
  in
  begin match run calls body with
  | Ok _ -> Alcotest.fail "default policy must reject residual execution"
  | Error message ->
      Alcotest.(check bool)
        "policy diagnostic" true
        (Test_support.contains ~needle:"opaque" message)
  end;
  let observation = run calls ~policy:Runner.permissive_policy body |> get in
  Alcotest.(check int) "permitted" 0 observation.exit_code;
  Alcotest.(check (list string))
    "shell capsule argv"
    [ "sh"; "-c"; "echo capsule" ]
    (List.hd !calls)

let test_residual_file_receives_original_arguments () =
  let calls = ref [] in
  let capsule =
    Ir.opaque_file ~path:"script.sh" ~interpreter:"sh"
      ~source:"printf '%s' \"$1\"" ~reason:"dynamic positional parameter"
  in
  let body =
    Ir.node ~id:"argument-capsule"
      ~guarantee:(Ir.Residual { reason = capsule.reason })
      (Ir.Opaque_capsule capsule)
  in
  let result =
    Runner.run_plan_with_inputs ~backend:(backend calls)
      ~policy:Runner.permissive_policy ~inputs:[] ~arguments:[ "hello world" ]
      (plan body)
    |> get
  in
  Alcotest.(check int) "exit" 0 result.exit_code;
  Alcotest.(check (list string))
    "file invocation preserves argv"
    [ "sh"; "script.sh"; "hello world" ]
    (List.hd !calls)

let test_residual_source_is_not_template_expanded () =
  let calls = ref [] in
  let source = "printf '%s' '${HOME}'" in
  let capsule =
    Ir.opaque ~interpreter:"sh" ~source ~reason:"quoted shell expansion"
  in
  let body =
    Ir.node ~id:"literal-source-capsule"
      ~guarantee:(Ir.Residual { reason = capsule.reason })
      (Ir.Opaque_capsule capsule)
  in
  let residual_plan =
    Ir.plan ~entrypoint:"main"
      [ Ir.task ~name:"main" ~environment:[ "HOME" ] ~body () ]
  in
  let result =
    Runner.run_plan_with_inputs ~backend:(backend calls)
      ~policy:Runner.permissive_policy
      ~inputs:[ ("HOME", "must-not-expand") ]
      residual_plan
    |> get
  in
  Alcotest.(check int) "exit" 0 result.exit_code;
  Alcotest.(check (list string))
    "capsule source bytes" [ "sh"; "-c"; source ] (List.hd !calls)

let test_positional_default_expansion () =
  let run_with arguments =
    let calls = ref [] in
    let body = node 40 (Ir.Exec (Ir.exec [ "emit"; "${1:-fallback}" ])) in
    let result =
      Runner.run_plan_with_inputs ~backend:(backend calls)
        ~policy:Runner.default_policy ~inputs:[] ~arguments (plan body)
      |> get
    in
    (result, List.rev !calls)
  in
  let absent, absent_calls = run_with [] in
  Alcotest.(check string) "absent uses default" "fallback" absent.stdout;
  Alcotest.(check (list (list string)))
    "absent argv"
    [ [ "emit"; "fallback" ] ]
    absent_calls;
  let present, present_calls = run_with [ "chosen" ] in
  Alcotest.(check string) "present uses argument" "chosen" present.stdout;
  Alcotest.(check (list (list string)))
    "present argv"
    [ [ "emit"; "chosen" ] ]
    present_calls;
  let empty, empty_calls = run_with [ "" ] in
  Alcotest.(check string) "empty uses default" "fallback" empty.stdout;
  Alcotest.(check (list (list string)))
    "empty argv"
    [ [ "emit"; "fallback" ] ]
    empty_calls

let test_template_dollar_escape () =
  let calls = ref [] in
  let body = node 41 (Ir.Exec (Ir.exec [ "emit"; "$${HOME}" ])) in
  let literal_plan =
    Ir.plan ~entrypoint:"main"
      [ Ir.task ~name:"main" ~environment:[ "HOME" ] ~body () ]
  in
  let result =
    Runner.run_plan_with_inputs ~backend:(backend calls)
      ~policy:Runner.default_policy
      ~inputs:[ ("HOME", "must-not-expand") ]
      literal_plan
    |> get
  in
  Alcotest.(check string) "literal template" "${HOME}" result.stdout;
  Alcotest.(check (list (list string)))
    "literal argv"
    [ [ "emit"; "${HOME}" ] ]
    (List.rev !calls)

let test_invalid_parameter_templates_are_rejected () =
  let expect_error ~needle template =
    let calls = ref [] in
    let body = node 42 (Ir.Exec (Ir.exec [ "emit"; template ])) in
    match run calls body with
    | Ok _ -> Alcotest.fail ("invalid template was executed: " ^ template)
    | Error message ->
        Alcotest.(check bool)
          ("diagnostic for " ^ template)
          true
          (Test_support.contains ~needle message);
        Alcotest.(check int) "no execution" 0 (List.length !calls)
  in
  expect_error ~needle:"unsupported parameter expression" "${MODE:=fallback}";
  expect_error ~needle:"invalid positional parameter"
    "${999999999999999999999999999999999999999999999999999999}"

let test_exec_capabilities_obey_policy () =
  let calls = ref [] in
  let run_command ?(policy = Runner.default_policy) argv =
    run calls ~policy (node 88 (Ir.Exec (Ir.exec argv)))
  in
  begin match run_command [ "cat"; "input.txt" ] with
  | Ok _ -> Alcotest.fail "known filesystem read bypassed policy"
  | Error message ->
      Alcotest.(check bool)
        "read diagnostic" true
        (Test_support.contains ~needle:"file read" message)
  end;
  begin match run_command [ "curl"; "https://example.invalid" ] with
  | Ok _ -> Alcotest.fail "known network command bypassed policy"
  | Error message ->
      Alcotest.(check bool)
        "network diagnostic" true
        (Test_support.contains ~needle:"network" message)
  end;
  let read_policy = { Runner.default_policy with allow_file_read = true } in
  begin match run_command ~policy:read_policy [ "cat"; "input.txt" ] with
  | Error message -> Alcotest.fail message
  | Ok _ -> ()
  end

let test_task_call () =
  let calls = ref [] in
  let helper =
    Ir.task ~name:"helper"
      ~body:(node 1 (Ir.Exec (Ir.exec [ "emit"; "task" ])))
      ()
  in
  let main =
    Ir.task ~name:"main"
      ~body:(node 2 (Ir.Task_call { task = "helper"; arguments = [] }))
      ()
  in
  let plan = Ir.plan ~entrypoint:"main" [ main; helper ] in
  let observation =
    Runner.run_plan ~backend:(backend calls) ~policy:Runner.default_policy plan
    |> get
  in
  Alcotest.(check string) "task output" "task" observation.stdout

let test_selected_node_retains_task_graph () =
  let calls = ref [] in
  let helper =
    Ir.task ~name:"helper"
      ~body:(node 1 (Ir.Exec (Ir.exec [ "emit"; "selected-task" ])))
      ()
  in
  let call =
    Ir.node ~id:"selected-call"
      ~guarantee:(Ir.Formal { basis = "runner-test" })
      (Ir.Task_call { task = "helper"; arguments = [] })
  in
  let main = Ir.task ~name:"main" ~body:call () in
  let original = Ir.plan ~entrypoint:"main" [ main; helper ] in
  let selected =
    match Cli.select_node original (Some "selected-call") with
    | Ok plan -> plan
    | Error message -> Alcotest.fail message
  in
  let observation =
    Runner.run_plan ~backend:(backend calls) ~policy:Runner.default_policy
      selected
    |> get
  in
  Alcotest.(check string) "helper output" "selected-task" observation.stdout

let test_foreach_binds_variable () =
  let calls = ref [] in
  let body =
    node 2
      (Ir.For_each
         {
           variable = "item";
           items = [ "alpha"; "beta" ];
           body = node 1 (Ir.Exec (Ir.exec [ "emit"; "${item}" ]));
         })
  in
  let observation = run calls body |> get in
  Alcotest.(check string) "combined output" "alphabeta" observation.stdout;
  Alcotest.(check (list (list string)))
    "bound calls"
    [ [ "emit"; "alpha" ]; [ "emit"; "beta" ] ]
    (List.rev !calls)

let test_task_arguments_and_environment () =
  let calls = ref [] in
  let environments = ref [] in
  let delegated = backend calls in
  let backend =
    {
      delegated with
      Runner.execute =
        (fun request ->
          environments := request.Runner.environment :: !environments;
          delegated.execute request);
    }
  in
  let helper =
    Ir.task ~name:"helper"
      ~inputs:[ Ir.{ name = "message"; value_type = Text } ]
      ~body:
        (node 1
           (Ir.Exec
              (Ir.exec
                 ~environment:[ ("VALUE", "${message}") ]
                 [ "emit"; "${message}" ])))
      ()
  in
  let main =
    Ir.task ~name:"main"
      ~body:
        (node 2
           (Ir.Task_call
              { task = "helper"; arguments = [ ("message", "bound") ] }))
      ()
  in
  let plan = Ir.plan ~entrypoint:"main" [ main; helper ] in
  let observation =
    Runner.run_plan ~backend ~policy:Runner.default_policy plan |> get
  in
  Alcotest.(check string) "argument" "bound" observation.stdout;
  Alcotest.(check (list (pair string string)))
    "environment"
    [ ("VALUE", "bound") ]
    (List.hd !environments)

let test_nested_task_inherits_global_environment () =
  let calls = ref [] in
  let helper =
    Ir.task ~name:"helper" ~environment:[ "HELPER_MODE" ]
      ~body:(node 51 (Ir.Exec (Ir.exec [ "emit"; "${HELPER_MODE}" ])))
      ()
  in
  let main =
    Ir.task ~name:"main"
      ~body:(node 52 (Ir.Task_call { task = "helper"; arguments = [] }))
      ()
  in
  let result =
    Runner.run_plan_with_inputs ~backend:(backend calls)
      ~policy:Runner.default_policy
      ~inputs:[ ("HELPER_MODE", "nested") ]
      (Ir.plan ~entrypoint:"main" [ main; helper ])
    |> get
  in
  Alcotest.(check string) "nested environment" "nested" result.stdout;
  Alcotest.(check (list (list string)))
    "nested argv"
    [ [ "emit"; "nested" ] ]
    (List.rev !calls)

let test_global_inputs_are_validated () =
  let calls = ref [] in
  let body = node 53 (Ir.Exec (Ir.exec [ "emit"; "ok" ])) in
  let simple_plan = plan body in
  begin match
    Runner.run_plan_with_inputs ~backend:(backend calls)
      ~policy:Runner.default_policy
      ~inputs:[ ("TYPO", "value") ]
      simple_plan
  with
  | Ok _ -> Alcotest.fail "unknown global input was ignored"
  | Error message ->
      Alcotest.(check bool)
        "unknown diagnostic" true
        (Test_support.contains ~needle:"unknown plan input TYPO" message)
  end;
  let environment_plan =
    Ir.plan ~entrypoint:"main"
      [ Ir.task ~name:"main" ~environment:[ "MODE" ] ~body () ]
  in
  begin match
    Runner.run_plan_with_inputs ~backend:(backend calls)
      ~policy:Runner.default_policy
      ~inputs:[ ("MODE", "one"); ("MODE", "two") ]
      environment_plan
  with
  | Ok _ -> Alcotest.fail "duplicate global input was accepted"
  | Error message ->
      Alcotest.(check bool)
        "duplicate diagnostic" true
        (Test_support.contains ~needle:"duplicate plan input MODE" message)
  end;
  Alcotest.(check int) "no execution" 0 (List.length !calls)

let test_missing_task_argument_fails_before_execution () =
  let calls = ref [] in
  let helper =
    Ir.task ~name:"helper"
      ~inputs:[ Ir.{ name = "required"; value_type = Text } ]
      ~body:(node 1 (Ir.Exec (Ir.exec [ "emit"; "${required}" ])))
      ()
  in
  let main =
    Ir.task ~name:"main"
      ~body:(node 2 (Ir.Task_call { task = "helper"; arguments = [] }))
      ()
  in
  let plan = Ir.plan ~entrypoint:"main" [ main; helper ] in
  begin match
    Runner.run_plan ~backend:(backend calls) ~policy:Runner.default_policy plan
  with
  | Ok _ -> Alcotest.fail "missing task input must fail"
  | Error message ->
      Alcotest.(check bool)
        "diagnostic" true
        (Test_support.contains ~needle:"missing argument required" message)
  end;
  Alcotest.(check int) "no execution" 0 (List.length !calls)

let test_secret_is_redacted_from_trace () =
  let calls = ref [] in
  let helper =
    Ir.task ~name:"helper"
      ~inputs:[ Ir.{ name = "token"; value_type = Secret Text } ]
      ~secrets:[ "token" ]
      ~body:(node 1 (Ir.Exec (Ir.exec [ "emit"; "${token}" ])))
      ()
  in
  let main =
    Ir.task ~name:"main"
      ~body:
        (node 2
           (Ir.Task_call
              { task = "helper"; arguments = [ ("token", "top-secret") ] }))
      ()
  in
  let plan = Ir.plan ~entrypoint:"main" [ main; helper ] in
  let observation =
    Runner.run_plan ~backend:(backend calls) ~policy:Runner.default_policy plan
    |> get
  in
  Alcotest.(check (list string))
    "backend receives value" [ "emit"; "top-secret" ] (List.hd !calls);
  match observation.trace with
  | [ Runner.Process (argv, 0) ] ->
      Alcotest.(check (list string))
        "trace redacted"
        [ "emit"; "<secret:token>" ]
        argv
  | _ -> Alcotest.fail "expected one process trace"

let test_secret_is_redacted_from_backend_errors () =
  let calls = ref [] in
  let failing_backend =
    {
      (backend calls) with
      execute =
        (fun request ->
          Error ("process failed for " ^ String.concat " " request.argv));
    }
  in
  let helper =
    Ir.task ~name:"helper"
      ~inputs:[ Ir.{ name = "token"; value_type = Secret Text } ]
      ~secrets:[ "token" ]
      ~body:(node 1 (Ir.Exec (Ir.exec [ "emit"; "${token}" ])))
      ()
  in
  let main =
    Ir.task ~name:"main"
      ~body:
        (node 2
           (Ir.Task_call
              { task = "helper"; arguments = [ ("token", "top-secret") ] }))
      ()
  in
  match
    Runner.run_plan ~backend:failing_backend ~policy:Runner.default_policy
      (Ir.plan ~entrypoint:"main" [ main; helper ])
  with
  | Ok _ -> Alcotest.fail "backend failure was ignored"
  | Error message ->
      Alcotest.(check bool)
        "secret absent" false
        (Test_support.contains ~needle:"top-secret" message);
      Alcotest.(check bool)
        "placeholder present" true
        (Test_support.contains ~needle:"<secret:token>" message)

let () =
  Alcotest.run "Internal runner"
    [
      ( "semantics",
        [
          Alcotest.test_case "pipeline" `Quick test_pipeline_stream;
          Alcotest.test_case "sequence" `Quick
            test_sequence_continues_after_failure;
          Alcotest.test_case "try/finally" `Quick
            test_finalizer_runs_and_preserves_failure;
          Alcotest.test_case "task call" `Quick test_task_call;
          Alcotest.test_case "selected task call" `Quick
            test_selected_node_retains_task_graph;
          Alcotest.test_case "foreach binding" `Quick
            test_foreach_binds_variable;
          Alcotest.test_case "task arguments" `Quick
            test_task_arguments_and_environment;
          Alcotest.test_case "nested task environment" `Quick
            test_nested_task_inherits_global_environment;
          Alcotest.test_case "global input validation" `Quick
            test_global_inputs_are_validated;
          Alcotest.test_case "missing input" `Quick
            test_missing_task_argument_fails_before_execution;
          Alcotest.test_case "secret redaction" `Quick
            test_secret_is_redacted_from_trace;
          Alcotest.test_case "secret error redaction" `Quick
            test_secret_is_redacted_from_backend_errors;
          Alcotest.test_case "residual arguments" `Quick
            test_residual_file_receives_original_arguments;
          Alcotest.test_case "residual source bytes" `Quick
            test_residual_source_is_not_template_expanded;
          Alcotest.test_case "positional default" `Quick
            test_positional_default_expansion;
          Alcotest.test_case "template dollar escape" `Quick
            test_template_dollar_escape;
          Alcotest.test_case "invalid parameter templates" `Quick
            test_invalid_parameter_templates_are_rejected;
          Alcotest.test_case "Exec capability policy" `Quick
            test_exec_capabilities_obey_policy;
        ] );
      ( "policy",
        [ Alcotest.test_case "residual capsule" `Quick test_residual_policy ] );
    ]
