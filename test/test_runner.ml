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

let powershell_invocation_plan ?(accepts_common_parameters = false) body =
  let invocation =
    Ir.
      {
        style = Powershell;
        accepts_common_parameters;
        parameters =
          [
            {
              input = "Name";
              position = Some 0;
              required = true;
              is_switch = false;
              default = None;
              validations = [];
            };
            {
              input = "Count";
              position = Some 1;
              required = false;
              is_switch = false;
              default = Some "2";
              validations = [];
            };
            {
              input = "Force";
              position = None;
              required = false;
              is_switch = true;
              default = Some "false";
              validations = [];
            };
          ];
      }
  in
  let task =
    Ir.task ~name:"main"
      ~inputs:
        [
          Ir.{ name = "Name"; value_type = Text };
          Ir.{ name = "Count"; value_type = Int };
          Ir.{ name = "Force"; value_type = Bool };
        ]
      ~invocation ~body ()
  in
  Ir.plan ~entrypoint:"main" [ task ]

let test_powershell_invocation_binding () =
  let run_with arguments =
    let calls = ref [] in
    let body =
      node 43 (Ir.Exec (Ir.exec [ "emit"; "${Name}:${Count}:${Force}" ]))
    in
    let result =
      Runner.run_plan_with_inputs ~backend:(backend calls)
        ~policy:Runner.default_policy ~inputs:[] ~arguments
        (powershell_invocation_plan body)
      |> get
    in
    (result, List.rev !calls)
  in
  let named, named_calls =
    run_with [ "-n"; "artifact"; "-Count:07"; "-Force" ]
  in
  Alcotest.(check string)
    "named and abbreviated parameters" "artifact:7:True" named.stdout;
  Alcotest.(check (list (list string)))
    "named argv"
    [ [ "emit"; "artifact:7:True" ] ]
    named_calls;
  let positional, positional_calls = run_with [ "archive" ] in
  Alcotest.(check string) "defaults" "archive:2:False" positional.stdout;
  Alcotest.(check (list (list string)))
    "positional argv"
    [ [ "emit"; "archive:2:False" ] ]
    positional_calls;
  let disabled, _ = run_with [ "-Name"; "archive"; "-Force:$false" ] in
  Alcotest.(check string)
    "explicit false switch" "archive:2:False" disabled.stdout;
  let input_calls = ref [] in
  let input_body =
    node 204 (Ir.Exec (Ir.exec [ "emit"; "${Name}:${Count}:${Force}" ]))
  in
  let from_case_insensitive_input =
    Runner.run_plan_with_inputs ~backend:(backend input_calls)
      ~policy:Runner.default_policy
      ~inputs:[ ("name", "artifact") ]
      ~arguments:[]
      (powershell_invocation_plan input_body)
    |> get
  in
  Alcotest.(check string)
    "case-insensitive explicit input" "artifact:2:False"
    from_case_insensitive_input.stdout;
  begin match
    Runner.run_plan_with_inputs ~backend:(backend input_calls)
      ~policy:Runner.default_policy
      ~inputs:[ ("Name", "one"); ("name", "two") ]
      ~arguments:[]
      (powershell_invocation_plan input_body)
  with
  | Ok _ -> Alcotest.fail "case-insensitive duplicate input was accepted"
  | Error message ->
      Alcotest.(check bool)
        "case-insensitive duplicate" true
        (Test_support.contains ~needle:"duplicate plan input" message)
  end;
  begin match
    Runner.run_plan_with_inputs ~backend:(backend input_calls)
      ~policy:Runner.default_policy
      ~inputs:[ ("name", "one") ]
      ~arguments:[ "-Name"; "two" ]
      (powershell_invocation_plan input_body)
  with
  | Ok _ -> Alcotest.fail "input and argv duplicate was accepted"
  | Error message ->
      Alcotest.(check bool)
        "input and argv duplicate" true
        (Test_support.contains ~needle:"specified more than once" message)
  end;
  let common_calls = ref [] in
  let with_common_parameters =
    Runner.run_plan_with_inputs ~backend:(backend common_calls)
      ~policy:Runner.default_policy ~inputs:[]
      ~arguments:
        [
          "artifact";
          "-Verbose:$false";
          "-ErrorAction";
          "Stop";
          "-OutBuffer";
          "2";
        ]
      (powershell_invocation_plan ~accepts_common_parameters:true input_body)
    |> get
  in
  Alcotest.(check string)
    "advanced script common parameters" "artifact:2:False"
    with_common_parameters.stdout;
  let expect_common_error arguments needle =
    match
      Runner.run_plan_with_inputs ~backend:(backend common_calls)
        ~policy:Runner.default_policy ~inputs:[] ~arguments
        (powershell_invocation_plan ~accepts_common_parameters:true input_body)
    with
    | Ok _ -> Alcotest.fail "invalid PowerShell common parameter was accepted"
    | Error message ->
        Alcotest.(check bool)
          "common parameter error" true
          (Test_support.contains ~needle message)
  in
  expect_common_error
    [ "artifact"; "-ErrorAction"; "Explode" ]
    "ActionPreference";
  expect_common_error
    [ "artifact"; "-Verbose"; "-vb:$false" ]
    "specified more than once";
  expect_common_error [ "artifact"; "-Verbose:0" ] "boolean switch"

let test_powershell_invocation_rejects_invalid_arguments () =
  let expect_error ~needle arguments =
    let calls = ref [] in
    let body =
      node 44 (Ir.Exec (Ir.exec [ "emit"; "${Name}:${Count}:${Force}" ]))
    in
    match
      Runner.run_plan_with_inputs ~backend:(backend calls)
        ~policy:Runner.default_policy ~inputs:[] ~arguments
        (powershell_invocation_plan body)
    with
    | Ok _ -> Alcotest.fail "invalid PowerShell invocation was executed"
    | Error message ->
        Alcotest.(check bool)
          "actionable binding error" true
          (Test_support.contains ~needle message);
        Alcotest.(check int) "no execution" 0 (List.length !calls)
  in
  expect_error ~needle:"missing mandatory" [];
  expect_error ~needle:"unknown PowerShell parameter" [ "-Unknown"; "value" ];
  expect_error ~needle:"unknown PowerShell parameter" [ "-Verbose" ];
  expect_error ~needle:"specified more than once"
    [ "-Name"; "one"; "-Name"; "two" ];
  expect_error ~needle:"Int32" [ "-Name"; "one"; "-Count"; "many" ]

let powershell_scalar_invocation_plan ?(required = true) ?default ~value_type
    ~is_switch () =
  let invocation =
    Ir.
      {
        style = Powershell;
        accepts_common_parameters = false;
        parameters =
          [
            {
              input = "Value";
              position = Some 0;
              required;
              is_switch;
              default;
              validations = [];
            };
          ];
      }
  in
  let body = node 205 (Ir.Exec (Ir.exec [ "emit"; "${Value}" ])) in
  Ir.plan ~entrypoint:"main"
    [
      Ir.task ~name:"main"
        ~inputs:[ Ir.{ name = "Value"; value_type } ]
        ~invocation ~body ();
    ]

let test_powershell_boolean_argument_grammar () =
  let run arguments =
    let calls = ref [] in
    let result =
      Runner.run_plan_with_inputs ~backend:(backend calls)
        ~policy:Runner.default_policy ~inputs:[] ~arguments
        (powershell_scalar_invocation_plan ~value_type:Ir.Bool ~is_switch:false
           ())
    in
    (result, List.rev !calls)
  in
  let explicit_false, false_calls = run [ "-Value:false" ] in
  let explicit_false = get explicit_false in
  Alcotest.(check string) "colon boolean" "False" explicit_false.stdout;
  Alcotest.(check (list (list string)))
    "colon boolean argv"
    [ [ "emit"; "False" ] ]
    false_calls;
  let expect_rejected ~needle arguments =
    match run arguments with
    | Ok _, _ -> Alcotest.fail "non-PowerShell boolean syntax was accepted"
    | Error message, calls ->
        Alcotest.(check bool)
          "boolean syntax diagnostic" true
          (Test_support.contains ~needle message);
        Alcotest.(check int)
          "boolean failure did not execute" 0 (List.length calls)
  in
  expect_rejected ~needle:"colon syntax" [ "-Value"; "false" ];
  expect_rejected ~needle:"colon syntax" [ "false" ];
  expect_rejected ~needle:"boolean literal" [ "-Value:0" ];
  let input_calls = ref [] in
  let from_typed_input =
    Runner.run_plan_with_inputs ~backend:(backend input_calls)
      ~policy:Runner.default_policy
      ~inputs:[ ("Value", "false") ]
      ~arguments:[]
      (powershell_scalar_invocation_plan ~value_type:Ir.Bool ~is_switch:false ())
    |> get
  in
  Alcotest.(check string) "typed input boolean" "False" from_typed_input.stdout;
  let default_calls = ref [] in
  let numeric_default =
    Runner.run_plan_with_inputs ~backend:(backend default_calls)
      ~policy:Runner.default_policy ~inputs:[] ~arguments:[]
      (powershell_scalar_invocation_plan ~required:false ~default:"1"
         ~value_type:Ir.Bool ~is_switch:false ())
    |> get
  in
  Alcotest.(check string)
    "numeric boolean default" "True" numeric_default.stdout

let test_powershell_int32_conversion () =
  let run value =
    let calls = ref [] in
    let result =
      Runner.run_plan_with_inputs ~backend:(backend calls)
        ~policy:Runner.default_policy ~inputs:[] ~arguments:[ "-Value"; value ]
        (powershell_scalar_invocation_plan ~value_type:Ir.Int ~is_switch:false
           ())
    in
    (result, List.rev !calls)
  in
  let expect_value input expected =
    let result, calls = run input in
    Alcotest.(check string) input expected (get result).stdout;
    Alcotest.(check (list (list string)))
      (input ^ " argv")
      [ [ "emit"; expected ] ]
      calls
  in
  expect_value "1e2" "100";
  expect_value "1.5" "2";
  expect_value "2.5" "2";
  expect_value "-1.5" "-2";
  expect_value "0b10" "2";
  expect_value "0x80000000" "-2147483648";
  let expect_rejected input =
    match run input with
    | Ok _, _ -> Alcotest.fail ("invalid Int32 was accepted: " ^ input)
    | Error message, calls ->
        Alcotest.(check bool)
          "Int32 diagnostic" true
          (Test_support.contains ~needle:"Int32" message);
        Alcotest.(check int)
          "Int32 failure did not execute" 0 (List.length calls)
  in
  expect_rejected "1_000";
  expect_rejected "2147483648"

let test_powershell_invocation_enforces_validations () =
  let calls = ref [] in
  let body = node 203 (Ir.Exec (Ir.exec [ "emit"; "${Required}" ])) in
  let parameter ?(required = false) ?default ?(validations = []) input position
      =
    Ir.
      {
        input;
        position = Some position;
        required;
        is_switch = false;
        default;
        validations;
      }
  in
  let invocation =
    Ir.
      {
        style = Powershell;
        accepts_common_parameters = false;
        parameters =
          [
            parameter ~required:true "Required" 0;
            parameter ~default:"Nightly"
              ~validations:
                [
                  String_set
                    { values = [ "Debug"; "Release" ]; ignore_case = true };
                ]
              "Mode" 1;
            parameter ~default:"9"
              ~validations:[ Int_range { minimum = 1; maximum = 5 } ]
              "Retry" 2;
            parameter ~default:"0"
              ~validations:[ Int_range { minimum = 0; maximum = 255 } ]
              "Mask" 3;
          ];
      }
  in
  let plan =
    Ir.plan ~entrypoint:"main"
      [
        Ir.task ~name:"main"
          ~inputs:
            [
              Ir.{ name = "Required"; value_type = Text };
              Ir.{ name = "Mode"; value_type = Text };
              Ir.{ name = "Retry"; value_type = Int };
              Ir.{ name = "Mask"; value_type = Int };
            ]
          ~invocation ~body ();
      ]
  in
  let expect_error arguments fragment =
    match
      Runner.run_plan_with_inputs ~backend:(backend calls)
        ~policy:Runner.default_policy ~inputs:[] ~arguments plan
    with
    | Ok _ -> Alcotest.fail "invalid validated argument was executed"
    | Error message ->
        Alcotest.(check bool)
          fragment true
          (Test_support.contains ~needle:fragment message)
  in
  expect_error [ "" ] "empty string";
  expect_error [ "ok"; "-Mode"; "Nightly" ] "ValidateSet";
  expect_error [ "ok"; "-Retry"; "6" ] "range";
  expect_error [ "ok"; "-Mask"; "256" ] "range";
  Alcotest.(check int)
    "validation failures were not executed" 0 (List.length !calls);
  begin match
    Runner.run_plan_with_inputs ~backend:(backend calls)
      ~policy:Runner.default_policy ~inputs:[] ~arguments:[ "ok" ] plan
  with
  | Error message ->
      Alcotest.fail
        ("PowerShell defaults must skip input validation: " ^ message)
  | Ok _ -> ()
  end;
  Alcotest.(check int)
    "invalid defaults are still executable" 1 (List.length !calls)

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

let test_typed_runtime_state_propagates_through_control_flow () =
  let set index name value =
    node index (Ir.Set_variable { name; value_type = Ir.Text; value })
  in
  let run_branch predicate =
    let calls = ref [] in
    let body =
      node 304
        (Ir.Sequence
           [
             node 302
               (Ir.Condition
                  {
                    predicate;
                    if_true = set 300 "mode" "release";
                    if_false = Some (set 301 "mode" "debug");
                  });
             set 305 "artifact" "build/${mode}";
             node 303 (Ir.Exec (Ir.exec [ "emit"; "${artifact}" ]));
           ])
    in
    let observation = run calls body |> get in
    (observation, List.rev !calls)
  in
  let successful, successful_calls =
    run_branch (node 306 (Ir.Exec (Ir.exec [ "emit"; "predicate" ])))
  in
  Alcotest.(check string)
    "true branch state" "predicatebuild/release" successful.stdout;
  Alcotest.(check (list (list string)))
    "true branch argv"
    [ [ "emit"; "predicate" ]; [ "emit"; "build/release" ] ]
    successful_calls;
  let failed, failed_calls =
    run_branch (node 307 (Ir.Exec (Ir.exec [ "fail"; "1" ])))
  in
  Alcotest.(check string) "false branch state" "build/debug" failed.stdout;
  Alcotest.(check (list (list string)))
    "false branch argv"
    [ [ "fail"; "1" ]; [ "emit"; "build/debug" ] ]
    failed_calls

let test_typed_runtime_state_checks_types_before_effects () =
  let calls = ref [] in
  let body =
    node 310
      (Ir.Sequence
         [
           node 308
             (Ir.Set_variable
                { name = "count"; value_type = Ir.Int; value = "not-an-int" });
           node 309 (Ir.Exec (Ir.exec [ "emit"; "${count}" ]));
         ])
  in
  begin match run calls body with
  | Ok _ -> Alcotest.fail "invalid typed state reached an external effect"
  | Error message ->
      Alcotest.(check bool)
        "typed state diagnostic" true
        (Test_support.contains ~needle:"integer" message)
  end;
  Alcotest.(check int) "no external effect" 0 (List.length !calls)

let test_stdout_capture_trims_and_isolates_subshell_state () =
  let calls = ref [] in
  let capture_body =
    node 330
      (Ir.Sequence
         [
           node 328
             (Ir.Set_variable
                { name = "inner"; value_type = Ir.Text; value = "leaked" });
           node 329 (Ir.Exec (Ir.exec [ "probe" ]));
         ])
  in
  let capture =
    node 331
      (Ir.Capture_stdout
         { name = "captured"; value_type = Ir.Text; body = capture_body })
  in
  let body =
    node 333
      (Ir.Sequence
         [
           capture;
           node 332
             (Ir.Exec (Ir.exec [ "consume"; "${captured}"; "${inner:-outer}" ]));
         ])
  in
  let capture_backend : Runner.backend =
    {
      (backend calls) with
      execute =
        (fun request ->
          calls := request.argv :: !calls;
          match request.argv with
          | [ "probe" ] ->
              Ok
                Runner.
                  {
                    exit_code = 7;
                    stdout = "first\nsecond\n\n";
                    stderr = "probe warning\n";
                  }
          | [ "consume"; "first\nsecond"; "outer" ] ->
              Ok Runner.{ exit_code = 0; stdout = "consumed\n"; stderr = "" }
          | argv -> Error ("unexpected argv: " ^ String.concat " " argv));
    }
  in
  let observation =
    Runner.run_plan ~backend:capture_backend ~policy:Runner.default_policy
      (plan body)
    |> get
  in
  Alcotest.(check (list (list string)))
    "capture and consumer argv"
    [ [ "probe" ]; [ "consume"; "first\nsecond"; "outer" ] ]
    (List.rev !calls);
  Alcotest.(check string)
    "captured stdout suppressed" "consumed\n" observation.stdout;
  Alcotest.(check string)
    "capture stderr forwarded" "probe warning\n" observation.stderr;
  let capture_only_calls = ref [] in
  let capture_only_backend =
    {
      capture_backend with
      execute =
        (fun request ->
          capture_only_calls := request.argv :: !capture_only_calls;
          Ok
            Runner.
              {
                exit_code = 7;
                stdout = "captured\n";
                stderr = "capture failed\n";
              });
    }
  in
  let captured_failure =
    Runner.run_plan ~backend:capture_only_backend ~policy:Runner.default_policy
      (plan capture)
    |> get
  in
  Alcotest.(check int) "capture status" 7 captured_failure.exit_code;
  Alcotest.(check string)
    "failed capture stdout suppressed" "" captured_failure.stdout;
  Alcotest.(check string)
    "failed capture stderr" "capture failed\n" captured_failure.stderr

let test_typed_runtime_secret_state_is_redacted () =
  let body =
    node 323
      (Ir.Sequence
         [
           node 320
             (Ir.Set_variable
                {
                  name = "token";
                  value_type = Ir.Secret Ir.Text;
                  value = "runtime-secret";
                });
           node 321 (Ir.Exec (Ir.exec [ "emit"; "${token}" ]));
         ])
  in
  let calls = ref [] in
  let observation = run calls body |> get in
  Alcotest.(check (list string))
    "backend receives secret"
    [ "emit"; "runtime-secret" ]
    (List.hd !calls);
  begin match observation.trace with
  | [ Runner.Process (argv, 0) ] ->
      Alcotest.(check (list string))
        "runtime secret trace redacted"
        [ "emit"; "<secret:token>" ]
        argv
  | _ -> Alcotest.fail "expected one runtime-secret process trace"
  end;
  let failing_backend =
    {
      (backend (ref [])) with
      execute =
        (fun request ->
          Error ("failed with " ^ String.concat " " request.Runner.argv));
    }
  in
  match
    Runner.run_plan ~backend:failing_backend ~policy:Runner.default_policy
      (plan body)
  with
  | Ok _ -> Alcotest.fail "runtime-secret backend failure was ignored"
  | Error message ->
      Alcotest.(check bool)
        "runtime secret absent from error" false
        (Test_support.contains ~needle:"runtime-secret" message);
      Alcotest.(check bool)
        "runtime secret placeholder in error" true
        (Test_support.contains ~needle:"<secret:token>" message)

let test_task_runtime_state_is_lexically_scoped () =
  let calls = ref [] in
  let set index value =
    node index (Ir.Set_variable { name = "mode"; value_type = Ir.Text; value })
  in
  let helper =
    Ir.task ~name:"helper"
      ~body:
        (node 314
           (Ir.Sequence
              [
                set 311 "helper";
                node 312 (Ir.Exec (Ir.exec [ "emit"; "${mode}" ]));
              ]))
      ()
  in
  let main =
    Ir.task ~name:"main"
      ~body:
        (node 318
           (Ir.Sequence
              [
                set 315 "main";
                node 316 (Ir.Task_call { task = "helper"; arguments = [] });
                node 317 (Ir.Exec (Ir.exec [ "emit"; "${mode}" ]));
              ]))
      ()
  in
  let observation =
    Runner.run_plan ~backend:(backend calls) ~policy:Runner.default_policy
      (Ir.plan ~entrypoint:"main" [ main; helper ])
    |> get
  in
  Alcotest.(check string) "scoped output" "helpermain" observation.stdout;
  Alcotest.(check (list (list string)))
    "scoped argv"
    [ [ "emit"; "helper" ]; [ "emit"; "main" ] ]
    (List.rev !calls)

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

let test_selected_node_retains_invocation_contract () =
  let calls = ref [] in
  let body =
    Ir.node ~id:"selected-parameter"
      ~guarantee:(Ir.Formal { basis = "runner-test" })
      (Ir.Exec (Ir.exec [ "emit"; "${Name}" ]))
  in
  let invocation =
    Ir.
      {
        style = Powershell;
        accepts_common_parameters = false;
        parameters =
          [
            {
              input = "Name";
              position = Some 0;
              required = true;
              is_switch = false;
              default = None;
              validations = [];
            };
          ];
      }
  in
  let main =
    Ir.task ~name:"main"
      ~inputs:[ Ir.{ name = "Name"; value_type = Text } ]
      ~invocation ~body ()
  in
  let original = Ir.plan ~entrypoint:"main" [ main ] in
  let selected =
    match Cli.select_node original (Some "selected-parameter") with
    | Ok plan -> plan
    | Error message -> Alcotest.fail message
  in
  let observation =
    Runner.run_plan_with_inputs ~backend:(backend calls)
      ~policy:Runner.default_policy ~inputs:[] ~arguments:[ "artifact" ]
      selected
    |> get
  in
  Alcotest.(check string) "bound selected input" "artifact" observation.stdout;
  Alcotest.(check (list (list string)))
    "selected argv"
    [ [ "emit"; "artifact" ] ]
    (List.rev !calls)

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
          Alcotest.test_case "selected invocation contract" `Quick
            test_selected_node_retains_invocation_contract;
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
          Alcotest.test_case "PowerShell invocation binding" `Quick
            test_powershell_invocation_binding;
          Alcotest.test_case "PowerShell invocation errors" `Quick
            test_powershell_invocation_rejects_invalid_arguments;
          Alcotest.test_case "PowerShell boolean argument grammar" `Quick
            test_powershell_boolean_argument_grammar;
          Alcotest.test_case "PowerShell Int32 conversion" `Quick
            test_powershell_int32_conversion;
          Alcotest.test_case "PowerShell invocation validations" `Quick
            test_powershell_invocation_enforces_validations;
          Alcotest.test_case "template dollar escape" `Quick
            test_template_dollar_escape;
          Alcotest.test_case "invalid parameter templates" `Quick
            test_invalid_parameter_templates_are_rejected;
          Alcotest.test_case "typed runtime state control flow" `Quick
            test_typed_runtime_state_propagates_through_control_flow;
          Alcotest.test_case "typed runtime state types" `Quick
            test_typed_runtime_state_checks_types_before_effects;
          Alcotest.test_case "stdout capture semantics" `Quick
            test_stdout_capture_trims_and_isolates_subshell_state;
          Alcotest.test_case "typed runtime state secrets" `Quick
            test_typed_runtime_secret_state_is_redacted;
          Alcotest.test_case "typed runtime state scope" `Quick
            test_task_runtime_state_is_lexically_scoped;
          Alcotest.test_case "Exec capability policy" `Quick
            test_exec_capabilities_obey_policy;
        ] );
      ( "policy",
        [ Alcotest.test_case "residual capsule" `Quick test_residual_policy ] );
    ]
