let read_json path = Yojson.Safe.from_file path

let require_member name json =
  match Yojson.Safe.Util.member name json with
  | `Null -> Alcotest.failf "%s is missing" name
  | value -> value

let test_schema filename expected_id () =
  let directory =
    match Sys.getenv_opt "DESHELL_SCHEMA_DIR" with
    | Some value -> value
    | None -> Alcotest.fail "DESHELL_SCHEMA_DIR is not set"
  in
  let schema = read_json (Filename.concat directory filename) in
  Alcotest.(check string)
    "$schema" "https://json-schema.org/draft/2020-12/schema"
    Yojson.Safe.Util.(schema |> member "$schema" |> to_string);
  Alcotest.(check string)
    "$id" expected_id
    Yojson.Safe.Util.(schema |> member "$id" |> to_string);
  ignore (require_member "type" schema)

let () =
  Alcotest.run "Published schemas"
    [
      ( "JSON Schema 2020-12",
        [
          Alcotest.test_case "Effect IR" `Quick
            (test_schema "effect-ir.schema.json"
               "https://deshell.dev/schema/effect-ir/v1");
          Alcotest.test_case "evidence" `Quick
            (test_schema "evidence.schema.json"
               "https://deshell.dev/schema/evidence/v1");
          Alcotest.test_case "adapter" `Quick
            (test_schema "adapter.schema.json"
               "https://deshell.dev/schema/adapter/v1");
        ] );
    ]
