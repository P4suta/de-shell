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

let rec find_operation operation_type = function
  | `Assoc fields as value ->
      begin match List.assoc_opt "properties" fields with
      | Some (`Assoc properties) ->
          begin match List.assoc_opt "type" properties with
          | Some (`Assoc type_fields)
            when List.assoc_opt "const" type_fields
                 = Some (`String operation_type) ->
              Some value
          | _ ->
              List.find_map
                (fun (_, child) -> find_operation operation_type child)
                fields
          end
      | _ ->
          List.find_map
            (fun (_, child) -> find_operation operation_type child)
            fields
      end
  | `List values -> List.find_map (find_operation operation_type) values
  | `Null | `Bool _ | `Int _ | `Intlit _ | `Float _ | `String _ -> None

let test_effect_ir_v3_contract () =
  let directory =
    match Sys.getenv_opt "DESHELL_SCHEMA_DIR" with
    | Some value -> value
    | None -> Alcotest.fail "DESHELL_SCHEMA_DIR is not set"
  in
  let schema = read_json (Filename.concat directory "effect-ir.schema.json") in
  Alcotest.(check int)
    "schema version" 3
    Yojson.Safe.Util.(
      schema |> member "properties" |> member "schema_version" |> member "const"
      |> to_int);
  let operation =
    match find_operation "set_variable" schema with
    | Some value -> value
    | None -> Alcotest.fail "set_variable operation is absent from Effect IR v3"
  in
  Alcotest.(check string)
    "state is scalar" "#/$defs/scalarValueType"
    Yojson.Safe.Util.(
      operation |> member "properties" |> member "value_type" |> member "$ref"
      |> to_string);
  let capture =
    match find_operation "capture_stdout" schema with
    | Some value -> value
    | None ->
        Alcotest.fail "capture_stdout operation is absent from Effect IR v3"
  in
  Alcotest.(check string)
    "capture result is text" "text"
    Yojson.Safe.Util.(
      capture |> member "properties" |> member "value_type" |> member "const"
      |> to_string);
  Alcotest.(check string)
    "capture body is typed IR" "#/$defs/node"
    Yojson.Safe.Util.(
      capture |> member "properties" |> member "body" |> member "$ref"
      |> to_string)

let () =
  Alcotest.run "Published schemas"
    [
      ( "JSON Schema 2020-12",
        [
          Alcotest.test_case "Effect IR" `Quick
            (test_schema "effect-ir.schema.json"
               "https://deshell.dev/schema/effect-ir/v3");
          Alcotest.test_case "Effect IR v3 state contract" `Quick
            test_effect_ir_v3_contract;
          Alcotest.test_case "evidence" `Quick
            (test_schema "evidence.schema.json"
               "https://deshell.dev/schema/evidence/v1");
          Alcotest.test_case "adapter" `Quick
            (test_schema "adapter.schema.json"
               "https://deshell.dev/schema/adapter/v1");
          Alcotest.test_case "corpus audit" `Quick
            (test_schema "corpus-audit.schema.json"
               "https://deshell.dev/schema/corpus-audit/v1");
        ] );
    ]
