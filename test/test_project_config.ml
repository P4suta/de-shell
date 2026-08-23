open Deshell

let test_default_configuration () =
  match Project_config.decode_string Project.default_project with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok config ->
      Alcotest.(check int) "version" 1 config.version;
      Alcotest.(check (list string)) "entrypoints" [] config.entrypoints;
      Alcotest.(check bool) "strict export" true config.export.strict;
      Alcotest.(check bool) "bridge disabled" false config.export.bridge

let test_invalid_policy_and_entrypoint_are_rejected () =
  let source =
    {|version = 1
entrypoints = ["../escape.sh"]

[policy]
host_write = "everywhere"
network = "open"
unknown_interpreter = "guess"

[sandbox]
mode = "host"

[export]
strict = false
bridge = true
|}
  in
  match Project_config.decode_string source with
  | Ok _ -> Alcotest.fail "unsafe project policy was accepted"
  | Error errors ->
      List.iter
        (fun needle ->
          Alcotest.(check bool)
            needle true
            (List.exists (Test_support.contains ~needle) errors))
        [ "entrypoint"; "host_write"; "network"; "unknown_interpreter"; "mode" ]

let () =
  Alcotest.run "Project configuration"
    [
      ( "decode",
        [
          Alcotest.test_case "default" `Quick test_default_configuration;
          Alcotest.test_case "unsafe values" `Quick
            test_invalid_policy_and_entrypoint_are_rejected;
        ] );
    ]
