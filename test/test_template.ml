open Deshell

let node index operation =
  Ir.node
    ~id:(Printf.sprintf "template-%d" index)
    ~guarantee:(Ir.Formal { basis = "template-test" })
    operation

let test_free_environment_variables () =
  let root =
    node 1
      (Ir.Sequence
         [
           node 2
             (Ir.Exec
                (Ir.exec
                   ~environment:[ ("TOKEN", "${TOKEN}") ]
                   ~working_directory:"${WORKDIR}"
                   [ "emit"; "${MODE:-release}"; "$${LITERAL}"; "${1:-x}" ]));
           node 3
             (Ir.For_each
                {
                  variable = "item";
                  items = [ "${ROOT}/one" ];
                  body =
                    node 4
                      (Ir.Exec (Ir.exec [ "emit"; "${item}"; "${TARGET}" ]));
                });
         ])
  in
  Alcotest.(check (list string))
    "sorted free variables"
    [ "MODE"; "ROOT"; "TARGET"; "TOKEN"; "WORKDIR" ]
    (Template.environment_variables root)

let test_opaque_source_is_not_a_template () =
  let capsule =
    Ir.opaque ~interpreter:"sh" ~source:"printf '%s' '${HOME}'"
      ~reason:"dynamic shell"
  in
  let root =
    Ir.node ~id:"opaque"
      ~guarantee:(Ir.Residual { reason = capsule.reason })
      (Ir.Opaque_capsule capsule)
  in
  Alcotest.(check (list string))
    "shell owns residual expansion" []
    (Template.environment_variables root)

let test_runtime_state_has_flow_sensitive_template_scope () =
  let set index name value =
    node index (Ir.Set_variable { name; value_type = Ir.Text; value })
  in
  let assigned_on_every_path =
    node 10
      (Ir.Sequence
         [
           node 11
             (Ir.Condition
                {
                  predicate = node 12 (Ir.Exec (Ir.exec [ "probe" ]));
                  if_true = set 13 "mode" "${ROOT}/release";
                  if_false = Some (set 14 "mode" "${ROOT}/debug");
                });
           node 15 (Ir.Exec (Ir.exec [ "emit"; "${mode}" ]));
         ])
  in
  Alcotest.(check (list string))
    "branch-local state is bound after every path" [ "ROOT" ]
    (Template.environment_variables assigned_on_every_path);
  let maybe_assigned =
    node 20
      (Ir.Sequence
         [
           node 21
             (Ir.Condition
                {
                  predicate = node 22 (Ir.Exec (Ir.exec [ "probe" ]));
                  if_true = set 23 "mode" "release";
                  if_false = None;
                });
           node 24 (Ir.Exec (Ir.exec [ "emit"; "${mode}" ]));
         ])
  in
  Alcotest.(check (list string))
    "one-path state remains a required external value" [ "mode" ]
    (Template.environment_variables maybe_assigned)

let () =
  Alcotest.run "IR templates"
    [
      ( "environment",
        [
          Alcotest.test_case "free variables" `Quick
            test_free_environment_variables;
          Alcotest.test_case "opaque source" `Quick
            test_opaque_source_is_not_a_template;
          Alcotest.test_case "flow-sensitive runtime state" `Quick
            test_runtime_state_has_flow_sensitive_template_scope;
        ] );
    ]
