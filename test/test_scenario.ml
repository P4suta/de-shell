open Deshell

let valid =
  {|name = "portable"
args = ["--mode", "check"]
timeout_ms = 1500

[environment]
MODE = "test"
EMPTY = ""

[[fixtures]]
path = "input/message.txt"
contents = "hello\n"
executable = false

[expect]
exit_code = 0
stdout = "ok\n"
stderr = ""

[[expect.files]]
path = "output/result.txt"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
|}

let test_decode_complete_scenario () =
  match Scenario.decode_string valid with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok scenario ->
      Alcotest.(check string) "name" "portable" scenario.name;
      Alcotest.(check (list string)) "args" [ "--mode"; "check" ] scenario.args;
      Alcotest.(check int) "timeout" 1500 scenario.timeout_ms;
      Alcotest.(check (list (pair string string)))
        "environment"
        [ ("EMPTY", ""); ("MODE", "test") ]
        scenario.environment;
      Alcotest.(check int) "fixture count" 1 (List.length scenario.fixtures);
      let fixture = List.hd scenario.fixtures in
      Alcotest.(check string) "fixture path" "input/message.txt" fixture.path;
      Alcotest.(check string) "fixture contents" "hello\n" fixture.contents;
      Alcotest.(check bool) "fixture mode" false fixture.executable;
      Alcotest.(check (option int)) "exit" (Some 0) scenario.expect.exit_code;
      Alcotest.(check (option string))
        "stdout" (Some "ok\n") scenario.expect.stdout;
      Alcotest.(check int) "expected file" 1 (List.length scenario.expect.files)

let test_defaults_and_comments () =
  let source =
    {|name = "small" # an inline comment
args = []

[environment]
VALUE = "# is data"

[expect]
exit_code = 0
|}
  in
  match Scenario.decode_string source with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok scenario ->
      Alcotest.(check int) "default timeout" 30000 scenario.timeout_ms;
      Alcotest.(check (option string))
        "stdout omitted" None scenario.expect.stdout;
      Alcotest.(check string)
        "quoted hash" "# is data"
        (List.assoc "VALUE" scenario.environment)

let test_rejects_escaping_fixture () =
  let source =
    {|name = "unsafe"
[[fixtures]]
path = "../host.txt"
contents = "no"
|}
  in
  match Scenario.decode_string source with
  | Ok _ -> Alcotest.fail "an escaping fixture must be rejected"
  | Error errors ->
      Alcotest.(check bool)
        "diagnostic" true
        (List.exists
           (fun error -> Test_support.contains ~needle:"project-relative" error)
           errors)

let test_rejects_duplicate_and_unknown_fields () =
  let source = {|name = "first"
name = "second"
surprise = true
|} in
  match Scenario.decode_string source with
  | Ok _ -> Alcotest.fail "invalid contract must be rejected"
  | Error errors ->
      Alcotest.(check bool)
        "duplicate" true
        (List.exists
           (fun error -> Test_support.contains ~needle:"duplicate" error)
           errors);
      Alcotest.(check bool)
        "unknown" true
        (List.exists
           (fun error -> Test_support.contains ~needle:"unknown" error)
           errors)

let test_directory_order_and_extension_filter () =
  Test_support.with_temp_dir @@ fun root ->
  Test_support.write_file (Filename.concat root "z.toml") "name = \"z\"\n";
  Test_support.write_file (Filename.concat root "a.toml") "name = \"a\"\n";
  Test_support.write_file (Filename.concat root "ignored.txt") "not toml\n";
  match Scenario.load_directory root with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok scenarios ->
      Alcotest.(check (list string))
        "deterministic" [ "a"; "z" ]
        (List.map (fun scenario -> scenario.Scenario.name) scenarios)

let () =
  Alcotest.run "Scenario contract"
    [
      ( "decode",
        [
          Alcotest.test_case "complete" `Quick test_decode_complete_scenario;
          Alcotest.test_case "defaults/comments" `Quick
            test_defaults_and_comments;
          Alcotest.test_case "escaping fixture" `Quick
            test_rejects_escaping_fixture;
          Alcotest.test_case "strict fields" `Quick
            test_rejects_duplicate_and_unknown_fields;
          Alcotest.test_case "directory" `Quick
            test_directory_order_and_extension_filter;
        ] );
    ]
