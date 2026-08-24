open Deshell

let span : Ir.source_span =
  {
    file = "script.sh";
    start_line = 1;
    start_column = 0;
    end_line = 1;
    end_column = 18;
    start_byte = 0;
    end_byte = 18;
  }

let sample_plan () =
  let command = Ir.exec [ "printf"; "%s\\n"; "hello" ] in
  let node =
    Ir.node ~id:"n-command"
      ~guarantee:(Ir.Formal { basis = "posix-literal-command" })
      ~source:span (Ir.Exec command)
  in
  let task = Ir.task ~name:"main" ~body:node () in
  Ir.plan ~entrypoint:"main" [ task ]

let test_round_trip () =
  let original = sample_plan () in
  let encoded = Ir_codec.encode_string original in
  match Ir_codec.decode_string encoded with
  | Error errors ->
      Alcotest.failf "decode failed: %s" (String.concat "; " errors)
  | Ok decoded ->
      Alcotest.(check bool)
        "same typed plan" true
        (Ir.equal_plan original decoded)

let test_invocation_round_trip () =
  let body =
    Ir.node ~id:"typed-invocation"
      ~guarantee:(Ir.Formal { basis = "powershell-parameter-binding-v1" })
      (Ir.Exec (Ir.exec [ "tool.exe"; "${Name}"; "${Count}"; "${Force}" ]))
  in
  let invocation =
    Ir.
      {
        style = Powershell;
        accepts_common_parameters = true;
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
              validations = [ Int_range { minimum = 1; maximum = 10 } ];
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
  let original = Ir.plan ~entrypoint:"main" [ task ] in
  let encoded = Ir_codec.encode_string original in
  match Ir_codec.decode_string encoded with
  | Error errors ->
      Alcotest.failf "invocation decode failed: %s" (String.concat "; " errors)
  | Ok decoded ->
      Alcotest.(check bool)
        "same typed invocation" true
        (Ir.equal_plan original decoded)

let test_unknown_fields_are_ignored () =
  let json = Ir_codec.encode_yojson (sample_plan ()) in
  let with_unknown =
    match json with
    | `Assoc fields ->
        `Assoc (("future_extension", `String "accepted") :: fields)
    | _ -> Alcotest.fail "plan must encode as an object"
  in
  match Ir_codec.decode_yojson with_unknown with
  | Error errors ->
      Alcotest.failf "unknown field rejected: %s" (String.concat "; " errors)
  | Ok decoded -> Alcotest.(check int) "schema version" 2 decoded.schema_version

let test_residual_requires_reason () =
  let capsule = Ir.opaque ~interpreter:"sh" ~source:"eval \"$x\"" ~reason:"" in
  let node =
    Ir.node ~id:"n-residual"
      ~guarantee:(Ir.Residual { reason = "" })
      (Ir.Opaque_capsule capsule)
  in
  let plan =
    Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:node () ]
  in
  match Ir.validate_plan plan with
  | Ok () -> Alcotest.fail "empty residual reasons must be rejected"
  | Error errors ->
      Alcotest.(check bool)
        "actionable error" true
        (List.exists (Test_support.contains ~needle:"residual reason") errors)

let test_v0_migration () =
  let legacy =
    {|{"version":0,"entrypoint":"main","commands":[["echo","hello"]]}|}
  in
  match Ir_codec.decode_string legacy with
  | Error errors ->
      Alcotest.failf "migration failed: %s" (String.concat "; " errors)
  | Ok plan -> (
      Alcotest.(check int) "migrated schema" 2 plan.schema_version;
      match (List.hd plan.tasks).body.operation with
      | Ir.Exec command ->
          Alcotest.(check (list string)) "argv" [ "echo"; "hello" ] command.argv
      | _ -> Alcotest.fail "legacy command was not migrated to Exec")

let test_v1_migration () =
  let legacy =
    match Ir_codec.encode_yojson (sample_plan ()) with
    | `Assoc fields ->
        let tasks =
          match List.assoc "tasks" fields with
          | `List tasks ->
              `List
                (List.map
                   (function
                     | `Assoc task_fields ->
                         `Assoc (List.remove_assoc "invocation" task_fields)
                     | value -> value)
                   tasks)
          | value -> value
        in
        `Assoc
          (("schema_version", `Int 1)
          :: ("tasks", tasks)
          :: (fields
             |> List.remove_assoc "schema_version"
             |> List.remove_assoc "tasks"))
    | _ -> Alcotest.fail "encoded plan must be an object"
  in
  match Ir_codec.decode_yojson legacy with
  | Error errors ->
      Alcotest.failf "v1 migration failed: %s" (String.concat "; " errors)
  | Ok plan ->
      Alcotest.(check int) "migrated schema" 2 plan.schema_version;
      Alcotest.(check bool)
        "missing invocation migrated to none" true
        ((List.hd plan.tasks).invocation = None)

let test_node_ids_are_globally_unique () =
  let make_task name =
    let body =
      Ir.node ~id:"shared-id"
        ~guarantee:(Ir.Formal { basis = "test" })
        (Ir.Exec (Ir.exec [ "true" ]))
    in
    Ir.task ~name ~body ()
  in
  let plan =
    Ir.plan ~entrypoint:"main" [ make_task "main"; make_task "helper" ]
  in
  match Ir.validate_plan plan with
  | Ok () -> Alcotest.fail "duplicate node IDs across tasks were accepted"
  | Error errors ->
      Alcotest.(check bool)
        "duplicate identified" true
        (List.exists (Test_support.contains ~needle:"duplicate node id") errors)

let test_task_call_target_must_exist () =
  let body =
    Ir.node ~id:"call-missing"
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Task_call { task = "missing"; arguments = [] })
  in
  let plan = Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ] in
  match Ir.validate_plan plan with
  | Ok () -> Alcotest.fail "missing task call target was accepted"
  | Error errors ->
      Alcotest.(check bool)
        "target identified" true
        (List.exists (Test_support.contains ~needle:"task not found") errors)

let test_task_bindings_and_secrets_are_validated () =
  let body =
    Ir.node ~id:"body"
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Exec (Ir.exec [ "true" ]))
  in
  let task =
    Ir.task ~name:"main"
      ~inputs:
        [
          Ir.{ name = "value"; value_type = Text };
          Ir.{ name = "value"; value_type = Text };
        ]
      ~secrets:[ "missing" ] ~body ()
  in
  match Ir.validate_plan (Ir.plan ~entrypoint:"main" [ task ]) with
  | Ok () -> Alcotest.fail "invalid task metadata was accepted"
  | Error errors ->
      Alcotest.(check bool)
        "duplicate binding" true
        (List.exists (Test_support.contains ~needle:"duplicate input") errors);
      Alcotest.(check bool)
        "secret declaration" true
        (List.exists (Test_support.contains ~needle:"secret missing") errors)

let test_environment_may_be_declared_secret () =
  let body =
    Ir.node ~id:"secret-environment-body"
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Exec (Ir.exec [ "emit"; "${BUILD_TOKEN}" ]))
  in
  let task =
    Ir.task ~name:"main" ~environment:[ "BUILD_TOKEN" ]
      ~secrets:[ "BUILD_TOKEN" ] ~body ()
  in
  match Ir.validate_plan (Ir.plan ~entrypoint:"main" [ task ]) with
  | Ok () -> ()
  | Error errors ->
      Alcotest.fail
        ("secret-backed environment was rejected: " ^ String.concat "; " errors)

let test_task_call_arguments_match_inputs () =
  let helper =
    Ir.task ~name:"helper"
      ~inputs:[ Ir.{ name = "expected"; value_type = Text } ]
      ~body:
        (Ir.node ~id:"helper-body"
           ~guarantee:(Ir.Formal { basis = "test" })
           (Ir.Exec (Ir.exec [ "true" ])))
      ()
  in
  let main =
    Ir.task ~name:"main"
      ~body:
        (Ir.node ~id:"call"
           ~guarantee:(Ir.Formal { basis = "test" })
           (Ir.Task_call
              {
                task = "helper";
                arguments = [ ("wrong", "x"); ("wrong", "y") ];
              }))
      ()
  in
  match Ir.validate_plan (Ir.plan ~entrypoint:"main" [ main; helper ]) with
  | Ok () -> Alcotest.fail "invalid call arguments were accepted"
  | Error errors ->
      Alcotest.(check bool)
        "duplicate argument" true
        (List.exists
           (Test_support.contains ~needle:"duplicate argument")
           errors);
      Alcotest.(check bool)
        "unknown argument" true
        (List.exists (Test_support.contains ~needle:"unknown argument") errors);
      Alcotest.(check bool)
        "missing argument" true
        (List.exists (Test_support.contains ~needle:"missing argument") errors)

let test_source_span_must_be_well_formed () =
  let invalid_span =
    Ir.
      {
        file = "script.sh";
        start_line = 2;
        start_column = 5;
        end_line = 1;
        end_column = 0;
        start_byte = 10;
        end_byte = 3;
      }
  in
  let body =
    Ir.node ~id:"span" ~source:invalid_span
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Exec (Ir.exec [ "true" ]))
  in
  match
    Ir.validate_plan
      (Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ])
  with
  | Ok () -> Alcotest.fail "reversed source span was accepted"
  | Error errors ->
      Alcotest.(check bool)
        "span diagnostic" true
        (List.exists (Test_support.contains ~needle:"source span") errors)

let invalid_errors plan =
  match Ir.validate_plan plan with
  | Ok () -> Alcotest.fail "invalid Effect IR was accepted"
  | Error errors -> errors

let has_error needle errors =
  Alcotest.(check bool)
    needle true
    (List.exists (Test_support.contains ~needle) errors)

let test_exec_contract_is_validated () =
  let body =
    Ir.node ~id:"exec"
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Exec
         (Ir.exec
            ~environment:[ ("TOKEN", "one"); ("TOKEN", "two"); ("", "x") ]
            ~working_directory:"" [ "  "; "argument" ]))
  in
  let errors =
    invalid_errors
      (Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ])
  in
  has_error "Exec executable" errors;
  has_error "duplicate Exec environment" errors;
  has_error "Exec environment name" errors;
  has_error "Exec working directory" errors

let test_guarantee_scenarios_are_unambiguous () =
  let body =
    Ir.node ~id:"guarantee"
      ~guarantee:(Ir.Exhaustive { scenarios = [ "smoke"; ""; "smoke" ] })
      (Ir.Exec (Ir.exec [ "true" ]))
  in
  let errors =
    invalid_errors
      (Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ])
  in
  has_error "duplicate guarantee scenario" errors;
  has_error "guarantee scenario" errors

let test_match_cases_are_unambiguous () =
  let branch id =
    Ir.node ~id
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Exec (Ir.exec [ "true" ]))
  in
  let body =
    Ir.node ~id:"match"
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Match
         {
           value = "mode";
           cases =
             [
               ("", branch "empty");
               ("debug", branch "one");
               ("debug", branch "two");
             ];
           default = None;
         })
  in
  let errors =
    invalid_errors
      (Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ])
  in
  has_error "match case label" errors;
  has_error "duplicate match case" errors

let test_capsule_requires_matching_residual_guarantee () =
  let capsule =
    Ir.opaque ~interpreter:"sh" ~source:"eval $x" ~reason:"dynamic eval"
  in
  let body =
    Ir.node ~id:"capsule"
      ~guarantee:(Ir.Formal { basis = "incorrect" })
      (Ir.Opaque_capsule capsule)
  in
  let errors =
    invalid_errors
      (Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ])
  in
  has_error "opaque capsule must use a residual guarantee" errors;
  let mismatched =
    Ir.node ~id:"mismatched"
      ~guarantee:(Ir.Residual { reason = "a different reason" })
      (Ir.Opaque_capsule capsule)
  in
  let errors =
    invalid_errors
      (Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:mismatched () ])
  in
  has_error "capsule residual reason must match" errors

let test_task_capabilities_are_unambiguous () =
  let body =
    Ir.node ~id:"body"
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Exec (Ir.exec [ "true" ]))
  in
  let task =
    Ir.task ~name:"main"
      ~platform_capabilities:[ "network"; ""; "network" ]
      ~body ()
  in
  let errors = invalid_errors (Ir.plan ~entrypoint:"main" [ task ]) in
  has_error "duplicate platform capability" errors;
  has_error "platform capability" errors

let test_invocation_contract_is_validated () =
  let body =
    Ir.node ~id:"body"
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Exec (Ir.exec [ "true" ]))
  in
  let invocation =
    Ir.
      {
        style = Powershell;
        accepts_common_parameters = true;
        parameters =
          [
            {
              input = "missing";
              position = Some (-1);
              required = true;
              is_switch = true;
              default = Some "also-invalid";
              validations =
                [
                  Allow_empty_string;
                  Allow_empty_string;
                  Not_null_or_empty;
                  String_set { values = []; ignore_case = true };
                  Int_range { minimum = 2; maximum = 1 };
                ];
            };
            {
              input = "MISSING";
              position = Some (-1);
              required = false;
              is_switch = false;
              default = None;
              validations = [];
            };
            {
              input = "Verbose";
              position = Some 2;
              required = false;
              is_switch = false;
              default = None;
              validations = [];
            };
          ];
      }
  in
  let task =
    Ir.task ~name:"main"
      ~inputs:
        [
          Ir.{ name = "Verbose"; value_type = Text };
          Ir.{ name = "verbose"; value_type = Text };
        ]
      ~invocation ~body ()
  in
  let errors = invalid_errors (Ir.plan ~entrypoint:"main" [ task ]) in
  has_error "unknown task input" errors;
  has_error "duplicate invocation parameter" errors;
  has_error "non-negative" errors;
  has_error "switch invocation parameter" errors;
  has_error "duplicate invocation validation" errors;
  has_error "conflicting empty-string validations" errors;
  has_error "string set" errors;
  has_error "minimum greater than maximum" errors;
  has_error "case-insensitive PowerShell input" errors;
  has_error "conflicts with a PowerShell common parameter" errors

let test_invocation_default_types_are_validated_before_execution () =
  let body =
    Ir.node ~id:"body"
      ~guarantee:(Ir.Formal { basis = "test" })
      (Ir.Exec (Ir.exec [ "true" ]))
  in
  let parameter input default validations =
    Ir.
      {
        input;
        position = None;
        required = false;
        is_switch = false;
        default = Some default;
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
            parameter "Count" "many" [];
            parameter "Retry" "9" [ Int_range { minimum = 1; maximum = 5 } ];
            parameter "Mode" "Nightly"
              [
                String_set
                  { values = [ "Debug"; "Release" ]; ignore_case = true };
              ];
            parameter "Enabled" "perhaps" [];
          ];
      }
  in
  let task =
    Ir.task ~name:"main"
      ~inputs:
        [
          Ir.{ name = "Count"; value_type = Int };
          Ir.{ name = "Retry"; value_type = Int };
          Ir.{ name = "Mode"; value_type = Text };
          Ir.{ name = "Enabled"; value_type = Bool };
        ]
      ~invocation ~body ()
  in
  let errors = invalid_errors (Ir.plan ~entrypoint:"main" [ task ]) in
  has_error "default for Count" errors;
  has_error "default for Enabled" errors;
  Alcotest.(check bool)
    "ValidateRange does not apply to a default" false
    (List.exists (Test_support.contains ~needle:"default for Retry") errors);
  Alcotest.(check bool)
    "ValidateSet does not apply to a default" false
    (List.exists (Test_support.contains ~needle:"default for Mode") errors)

let () =
  Alcotest.run "Effect IR"
    [
      ( "codec",
        [
          Alcotest.test_case "round trip" `Quick test_round_trip;
          Alcotest.test_case "invocation round trip" `Quick
            test_invocation_round_trip;
          Alcotest.test_case "unknown fields" `Quick
            test_unknown_fields_are_ignored;
          Alcotest.test_case "v0 migration" `Quick test_v0_migration;
          Alcotest.test_case "v1 migration" `Quick test_v1_migration;
        ] );
      ( "validation",
        [
          Alcotest.test_case "residual reason" `Quick
            test_residual_requires_reason;
          Alcotest.test_case "global node IDs" `Quick
            test_node_ids_are_globally_unique;
          Alcotest.test_case "task call target" `Quick
            test_task_call_target_must_exist;
          Alcotest.test_case "task bindings/secrets" `Quick
            test_task_bindings_and_secrets_are_validated;
          Alcotest.test_case "secret environment" `Quick
            test_environment_may_be_declared_secret;
          Alcotest.test_case "task call arguments" `Quick
            test_task_call_arguments_match_inputs;
          Alcotest.test_case "source span" `Quick
            test_source_span_must_be_well_formed;
          Alcotest.test_case "Exec contract" `Quick
            test_exec_contract_is_validated;
          Alcotest.test_case "guarantee scenarios" `Quick
            test_guarantee_scenarios_are_unambiguous;
          Alcotest.test_case "match cases" `Quick
            test_match_cases_are_unambiguous;
          Alcotest.test_case "capsule guarantee" `Quick
            test_capsule_requires_matching_residual_guarantee;
          Alcotest.test_case "task capabilities" `Quick
            test_task_capabilities_are_unambiguous;
          Alcotest.test_case "invocation contract" `Quick
            test_invocation_contract_is_validated;
          Alcotest.test_case "invocation default types" `Quick
            test_invocation_default_types_are_validated_before_execution;
        ] );
    ]
