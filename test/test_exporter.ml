open Deshell

let literal_plan () =
  let command =
    Ir.node ~id:"exec-1"
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Exec (Ir.exec [ "printf"; "hello world" ]))
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:command () ]

let residual_plan () =
  let capsule =
    Ir.opaque ~interpreter:"sh" ~source:"echo $VALUE" ~reason:"dynamic"
  in
  let node =
    Ir.node ~id:"opaque-1"
      ~guarantee:(Ir.Residual { reason = "dynamic" })
      (Ir.Opaque_capsule capsule)
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:node () ]

let environment_plan () =
  let node =
    Ir.node ~id:"env-1"
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Exec (Ir.exec ~environment:[ ("MODE", "release") ] [ "build" ]))
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:node () ]

let typed_invocation_plan () =
  let node =
    Ir.node ~id:"typed-input"
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Exec (Ir.exec [ "build"; "${Name}" ]))
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
  Ir.plan ~entrypoint:"main"
    [
      Ir.task ~name:"main"
        ~inputs:[ Ir.{ name = "Name"; value_type = Text } ]
        ~invocation ~body:node ();
    ]

let mixed_plan () =
  let literal =
    Ir.node ~id:"exec-before"
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Exec (Ir.exec [ "printf"; "before" ]))
  in
  let capsule =
    Ir.opaque ~interpreter:"sh" ~source:"echo $VALUE" ~reason:"dynamic"
  in
  let residual =
    Ir.node ~id:"opaque-middle"
      ~guarantee:(Ir.Residual { reason = "dynamic" })
      (Ir.Opaque_capsule capsule)
  in
  let root =
    Ir.node ~id:"sequence-root"
      ~guarantee:(Ir.Residual { reason = "contains residual behavior" })
      (Ir.Sequence [ literal; residual ])
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:root () ]

let sequence_plan () =
  let command id value =
    Ir.node ~id
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Exec (Ir.exec [ "printf"; value ]))
  in
  let root =
    Ir.node ~id:"sequence"
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Sequence [ command "first" "one"; command "second" "two" ])
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:root () ]

let runtime_state_plan () =
  let root =
    Ir.node ~id:"state-sequence"
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Sequence
         [
           Ir.node ~id:"state-mode"
             ~guarantee:(Ir.Formal { basis = "export-test" })
             (Ir.Set_variable
                { name = "mode"; value_type = Ir.Text; value = "release" });
           Ir.node ~id:"state-use"
             ~guarantee:(Ir.Formal { basis = "export-test" })
             (Ir.Exec (Ir.exec [ "build"; "${mode}" ]));
         ])
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:root () ]

let stdout_capture_plan () =
  let capture_body =
    Ir.node ~id:"capture-command"
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Exec (Ir.exec [ "probe" ]))
  in
  let root =
    Ir.node ~id:"capture-revision"
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Capture_stdout
         { name = "revision"; value_type = Ir.Text; body = capture_body })
  in
  Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:root () ]

let export target =
  match Exporter.export ~target ~bridge:false (literal_plan ()) with
  | Ok artifact -> artifact
  | Error message -> Alcotest.fail message

let test_dagger () =
  let artifact = export Exporter.Dagger in
  Alcotest.(check string) "filename" "deshell.dagger.ts" artifact.filename;
  Alcotest.(check bool)
    "argv remains structured" true
    (Test_support.contains ~needle:{|withExec(["printf","hello world"]|}
       artifact.content);
  Alcotest.(check bool)
    "base image is immutable" true
    (Test_support.contains ~needle:"alpine@sha256:" artifact.content
    && not (Test_support.contains ~needle:"alpine:3.22" artifact.content))

let test_dagger_sequence_collects_every_stdout () =
  match
    Exporter.export ~target:Exporter.Dagger ~bridge:false (sequence_plan ())
  with
  | Error message -> Alcotest.fail message
  | Ok artifact ->
      let needle = "output += await container.stdout();" in
      let first = Test_support.contains ~needle artifact.content in
      let second =
        match String.index_opt artifact.content ';' with
        | None -> false
        | Some _ ->
            let needle_length = String.length needle in
            let rec count index found =
              if index + needle_length > String.length artifact.content then
                found
              else if String.sub artifact.content index needle_length = needle
              then count (index + needle_length) (found + 1)
              else count (index + 1) found
            in
            count 0 0 = 2
      in
      Alcotest.(check bool) "each stdout collected" true (first && second);
      Alcotest.(check bool)
        "combined output returned" true
        (Test_support.contains ~needle:"return output;" artifact.content)

let test_nu () =
  let artifact = export Exporter.Nu in
  Alcotest.(check string) "filename" "deshell.nu" artifact.filename;
  Alcotest.(check bool)
    "module command" true
    (Test_support.contains ~needle:"export def main" artifact.content)

let test_cwl () =
  let artifact = export Exporter.Cwl in
  Alcotest.(check string) "filename" "deshell.cwl" artifact.filename;
  let document = Yojson.Safe.from_string artifact.content in
  Alcotest.(check string)
    "version" "v1.2"
    Yojson.Safe.Util.(document |> member "cwlVersion" |> to_string);
  Alcotest.(check (list string))
    "base command" [ "printf" ]
    Yojson.Safe.Util.(
      document |> member "baseCommand" |> to_list |> filter_string)

let test_every_artifact_self_validates () =
  List.iter
    (fun target ->
      let artifact = export target in
      match Exporter.validate_artifact ~target artifact with
      | Ok () -> ()
      | Error errors -> Alcotest.fail (String.concat "; " errors))
    [ Exporter.Internal; Exporter.Dagger; Exporter.Nu; Exporter.Cwl ];
  let malformed =
    Exporter.
      {
        filename = "deshell.cwl";
        media_type = "application/cwl+json";
        content = {|{"cwlVersion":"v1.2","class":"Workflow"}|};
      }
  in
  match Exporter.validate_artifact ~target:Exporter.Cwl malformed with
  | Ok () -> Alcotest.fail "malformed CWL was accepted"
  | Error errors ->
      Alcotest.(check bool)
        "structural diagnostic" true
        (List.exists (Test_support.contains ~needle:"CommandLineTool") errors)

let test_strict_residual_rejection () =
  match
    Exporter.export ~target:Exporter.Cwl ~bridge:false (residual_plan ())
  with
  | Ok _ -> Alcotest.fail "strict exporter silently accepted a residual"
  | Error message ->
      Alcotest.(check bool)
        "node identified" true
        (Test_support.contains ~needle:"opaque-1" message)

let test_bridge_is_explicit () =
  match
    Exporter.export ~target:Exporter.Cwl ~bridge:true (residual_plan ())
  with
  | Error message -> Alcotest.fail message
  | Ok artifact ->
      Alcotest.(check bool)
        "bridge invokes internal plan" true
        (Test_support.contains ~needle:"deshell" artifact.content
        && Test_support.contains ~needle:"--allow-residual" artifact.content
        && not (Test_support.contains ~needle:"--node" artifact.content))

let test_bridge_preserves_composite_plan () =
  match Exporter.export ~target:Exporter.Nu ~bridge:true (mixed_plan ()) with
  | Error message -> Alcotest.fail message
  | Ok artifact ->
      Alcotest.(check bool)
        "whole plan delegated" true
        (Test_support.contains ~needle:"--allow-residual" artifact.content
        && not (Test_support.contains ~needle:"opaque-middle" artifact.content)
        )

let test_strict_export_rejects_dropped_environment () =
  match
    Exporter.export ~target:Exporter.Nu ~bridge:false (environment_plan ())
  with
  | Ok _ -> Alcotest.fail "strict exporter silently dropped Exec.environment"
  | Error message ->
      Alcotest.(check bool)
        "node and capability identified" true
        (Test_support.contains ~needle:"env-1" message
        && Test_support.contains ~needle:"environment" message)

let test_strict_export_rejects_dropped_task_interface () =
  List.iter
    (fun target ->
      match
        Exporter.export ~target ~bridge:false (typed_invocation_plan ())
      with
      | Ok _ -> Alcotest.fail "strict exporter silently dropped task inputs"
      | Error message ->
          Alcotest.(check bool)
            "task interface identified" true
            (Test_support.contains ~needle:"main" message
            && Test_support.contains ~needle:"invocation" message))
    [ Exporter.Dagger; Exporter.Nu; Exporter.Cwl ];
  match
    Exporter.export ~target:Exporter.Nu ~bridge:true (typed_invocation_plan ())
  with
  | Ok _ ->
      Alcotest.fail "bridge claimed to preserve an invocation it cannot forward"
  | Error message ->
      Alcotest.(check bool)
        "bridge limitation is explicit" true
        (Test_support.contains ~needle:"invocation" message)

let test_runtime_state_requires_explicit_bridge () =
  [ Exporter.Dagger; Exporter.Nu; Exporter.Cwl ]
  |> List.iter (fun target ->
      begin match
        Exporter.export ~target ~bridge:false (runtime_state_plan ())
      with
      | Ok _ -> Alcotest.fail "strict exporter silently dropped runtime state"
      | Error message ->
          Alcotest.(check bool)
            "state node identified" true
            (Test_support.contains ~needle:"state-mode" message
            && Test_support.contains ~needle:"set_variable" message)
      end;
      match Exporter.export ~target ~bridge:true (runtime_state_plan ()) with
      | Error message -> Alcotest.fail message
      | Ok artifact ->
          Alcotest.(check bool)
            "state plan delegated whole" true
            (Test_support.contains ~needle:"deshell" artifact.content
            && Test_support.contains ~needle:"--allow-residual" artifact.content
            ))

let test_stdout_capture_requires_explicit_bridge () =
  [ Exporter.Dagger; Exporter.Nu; Exporter.Cwl ]
  |> List.iter (fun target ->
      begin match
        Exporter.export ~target ~bridge:false (stdout_capture_plan ())
      with
      | Ok _ -> Alcotest.fail "strict exporter silently dropped stdout capture"
      | Error message ->
          Alcotest.(check bool)
            "capture node identified" true
            (Test_support.contains ~needle:"capture-revision" message
            && Test_support.contains ~needle:"capture_stdout" message)
      end;
      match Exporter.export ~target ~bridge:true (stdout_capture_plan ()) with
      | Error message -> Alcotest.fail message
      | Ok artifact ->
          Alcotest.(check bool)
            "capture plan delegated whole" true
            (Test_support.contains ~needle:"deshell" artifact.content
            && Test_support.contains ~needle:"--allow-residual" artifact.content
            ))

let test_cwl_empty_arguments_are_an_array () =
  let node =
    Ir.node ~id:"no-args"
      ~guarantee:(Ir.Formal { basis = "export-test" })
      (Ir.Exec (Ir.exec [ "true" ]))
  in
  let plan =
    Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:node () ]
  in
  match Exporter.export ~target:Exporter.Cwl ~bridge:false plan with
  | Error message -> Alcotest.fail message
  | Ok artifact ->
      Alcotest.(check bool)
        "empty list" true
        Yojson.Safe.Util.(
          Yojson.Safe.from_string artifact.content
          |> member "arguments" |> to_list = [])

let () =
  Alcotest.run "Strict exporters"
    [
      ( "targets",
        [
          Alcotest.test_case "Dagger" `Quick test_dagger;
          Alcotest.test_case "Dagger sequence stdout" `Quick
            test_dagger_sequence_collects_every_stdout;
          Alcotest.test_case "Nushell" `Quick test_nu;
          Alcotest.test_case "CWL 1.2" `Quick test_cwl;
          Alcotest.test_case "self validation" `Quick
            test_every_artifact_self_validates;
        ] );
      ( "capabilities",
        [
          Alcotest.test_case "strict residual" `Quick
            test_strict_residual_rejection;
          Alcotest.test_case "explicit bridge" `Quick test_bridge_is_explicit;
          Alcotest.test_case "composite bridge" `Quick
            test_bridge_preserves_composite_plan;
          Alcotest.test_case "environment capability" `Quick
            test_strict_export_rejects_dropped_environment;
          Alcotest.test_case "task invocation capability" `Quick
            test_strict_export_rejects_dropped_task_interface;
          Alcotest.test_case "runtime state bridge" `Quick
            test_runtime_state_requires_explicit_bridge;
          Alcotest.test_case "stdout capture bridge" `Quick
            test_stdout_capture_requires_explicit_bridge;
          Alcotest.test_case "CWL empty arguments" `Quick
            test_cwl_empty_arguments_are_an_array;
        ] );
    ]
