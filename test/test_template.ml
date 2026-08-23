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

let () =
  Alcotest.run "IR templates"
    [
      ( "environment",
        [
          Alcotest.test_case "free variables" `Quick
            test_free_environment_variables;
          Alcotest.test_case "opaque source" `Quick
            test_opaque_source_is_not_a_template;
        ] );
    ]
