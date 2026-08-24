let ( let* ) result continuation =
  match result with Ok value -> continuation value | Error _ as error -> error

let error message = Error [ message ]
let errorf format = Printf.ksprintf error format

let as_object context = function
  | `Assoc fields -> Ok fields
  | _ -> errorf "%s must be a JSON object" context

let as_string context = function
  | `String value -> Ok value
  | _ -> errorf "%s must be a string" context

let as_int context = function
  | `Int value -> Ok value
  | _ -> errorf "%s must be an integer" context

let as_bool context = function
  | `Bool value -> Ok value
  | _ -> errorf "%s must be a boolean" context

let required name fields =
  match List.assoc_opt name fields with
  | Some value -> Ok value
  | None -> errorf "missing required field: %s" name

let optional name fields = List.assoc_opt name fields

let rec decode_list context decoder = function
  | `List values ->
      let rec loop index accumulator = function
        | [] -> Ok (List.rev accumulator)
        | value :: rest ->
            let* decoded =
              decoder (Printf.sprintf "%s[%d]" context index) value
            in
            loop (index + 1) (decoded :: accumulator) rest
      in
      loop 0 [] values
  | _ -> errorf "%s must be an array" context

let decode_string_list context json = decode_list context as_string json

let encode_value_type value_type =
  let rec encode = function
    | Ir.Bytes -> `String "bytes"
    | Ir.Text -> `String "text"
    | Ir.Bool -> `String "bool"
    | Ir.Int -> `String "int"
    | Ir.Path -> `String "path"
    | Ir.Byte_stream -> `String "byte_stream"
    | Ir.List item -> `Assoc [ ("list", encode item) ]
    | Ir.Record fields ->
        `Assoc
          [
            ( "record",
              `Assoc
                (List.map
                   (fun (name, field_type) -> (name, encode field_type))
                   fields) );
          ]
    | Ir.Secret item -> `Assoc [ ("secret", encode item) ]
    | Ir.Object_stream item -> `Assoc [ ("object_stream", encode item) ]
  in
  encode value_type

let rec decode_value_type context = function
  | `String "bytes" -> Ok Ir.Bytes
  | `String "text" -> Ok Ir.Text
  | `String "bool" -> Ok Ir.Bool
  | `String "int" -> Ok Ir.Int
  | `String "path" -> Ok Ir.Path
  | `String "byte_stream" -> Ok Ir.Byte_stream
  | `String other -> errorf "%s has unknown value type %S" context other
  | `Assoc [ ("list", item) ] ->
      let* item = decode_value_type (context ^ ".list") item in
      Ok (Ir.List item)
  | `Assoc [ ("secret", item) ] ->
      let* item = decode_value_type (context ^ ".secret") item in
      Ok (Ir.Secret item)
  | `Assoc [ ("object_stream", item) ] ->
      let* item = decode_value_type (context ^ ".object_stream") item in
      Ok (Ir.Object_stream item)
  | `Assoc [ ("record", `Assoc fields) ] ->
      let rec loop accumulator = function
        | [] -> Ok (Ir.Record (List.rev accumulator))
        | (name, field_type) :: rest ->
            let* field_type =
              decode_value_type (context ^ ".record." ^ name) field_type
            in
            loop ((name, field_type) :: accumulator) rest
      in
      loop [] fields
  | _ -> errorf "%s is not a valid value type" context

let encode_span (span : Ir.source_span) =
  `Assoc
    [
      ("file", `String span.file);
      ("start_line", `Int span.start_line);
      ("start_column", `Int span.start_column);
      ("end_line", `Int span.end_line);
      ("end_column", `Int span.end_column);
      ("start_byte", `Int span.start_byte);
      ("end_byte", `Int span.end_byte);
    ]

let decode_span context json =
  let* fields = as_object context json in
  let* file_json = required "file" fields in
  let* file = as_string (context ^ ".file") file_json in
  let* start_line_json = required "start_line" fields in
  let* start_line = as_int (context ^ ".start_line") start_line_json in
  let* start_column_json = required "start_column" fields in
  let* start_column = as_int (context ^ ".start_column") start_column_json in
  let* end_line_json = required "end_line" fields in
  let* end_line = as_int (context ^ ".end_line") end_line_json in
  let* end_column_json = required "end_column" fields in
  let* end_column = as_int (context ^ ".end_column") end_column_json in
  let* start_byte_json = required "start_byte" fields in
  let* start_byte = as_int (context ^ ".start_byte") start_byte_json in
  let* end_byte_json = required "end_byte" fields in
  let* end_byte = as_int (context ^ ".end_byte") end_byte_json in
  Ok
    Ir.
      {
        file;
        start_line;
        start_column;
        end_line;
        end_column;
        start_byte;
        end_byte;
      }

let encode_guarantee = function
  | Ir.Formal { basis } ->
      `Assoc [ ("level", `String "formal"); ("basis", `String basis) ]
  | Ir.Exhaustive { scenarios } ->
      `Assoc
        [
          ("level", `String "exhaustive");
          ("scenarios", `List (List.map (fun value -> `String value) scenarios));
        ]
  | Ir.Differential { scenarios; observation_digest } ->
      `Assoc
        [
          ("level", `String "differential");
          ("scenarios", `List (List.map (fun value -> `String value) scenarios));
          ("observation_digest", `String observation_digest);
        ]
  | Ir.Residual { reason } ->
      `Assoc [ ("level", `String "residual"); ("reason", `String reason) ]

let decode_guarantee context json =
  let* fields = as_object context json in
  let* level_json = required "level" fields in
  let* level = as_string (context ^ ".level") level_json in
  match level with
  | "formal" ->
      let* basis_json = required "basis" fields in
      let* basis = as_string (context ^ ".basis") basis_json in
      Ok (Ir.Formal { basis })
  | "exhaustive" ->
      let* scenarios_json = required "scenarios" fields in
      let* scenarios =
        decode_string_list (context ^ ".scenarios") scenarios_json
      in
      Ok (Ir.Exhaustive { scenarios })
  | "differential" ->
      let* scenarios_json = required "scenarios" fields in
      let* scenarios =
        decode_string_list (context ^ ".scenarios") scenarios_json
      in
      let* digest_json = required "observation_digest" fields in
      let* observation_digest =
        as_string (context ^ ".observation_digest") digest_json
      in
      Ok (Ir.Differential { scenarios; observation_digest })
  | "residual" ->
      let* reason_json = required "reason" fields in
      let* reason = as_string (context ^ ".reason") reason_json in
      Ok (Ir.Residual { reason })
  | other -> errorf "%s has unknown guarantee level %S" context other

let encode_pairs pairs =
  `Assoc (List.map (fun (name, value) -> (name, `String value)) pairs)

let decode_pairs context = function
  | `Assoc pairs ->
      let rec loop accumulator = function
        | [] -> Ok (List.rev accumulator)
        | (name, value) :: rest ->
            let* value = as_string (context ^ "." ^ name) value in
            loop ((name, value) :: accumulator) rest
      in
      loop [] pairs
  | _ -> errorf "%s must be an object of string values" context

let rec encode_node (node : Ir.node) =
  let operation =
    match node.operation with
    | Ir.Exec command ->
        `Assoc
          [
            ("type", `String "exec");
            ("argv", `List (List.map (fun value -> `String value) command.argv));
            ("environment", encode_pairs command.environment);
            ( "working_directory",
              match command.working_directory with
              | None -> `Null
              | Some value -> `String value );
          ]
    | Ir.Pipeline nodes ->
        `Assoc
          [
            ("type", `String "pipeline");
            ("nodes", `List (List.map encode_node nodes));
          ]
    | Ir.Sequence nodes ->
        `Assoc
          [
            ("type", `String "sequence");
            ("nodes", `List (List.map encode_node nodes));
          ]
    | Ir.Parallel nodes ->
        `Assoc
          [
            ("type", `String "parallel");
            ("nodes", `List (List.map encode_node nodes));
          ]
    | Ir.Condition { predicate; if_true; if_false } ->
        `Assoc
          [
            ("type", `String "condition");
            ("predicate", encode_node predicate);
            ("if_true", encode_node if_true);
            ("if_false", Option.fold ~none:`Null ~some:encode_node if_false);
          ]
    | Ir.Match { value; cases; default } ->
        `Assoc
          [
            ("type", `String "match");
            ("value", `String value);
            ( "cases",
              `List
                (List.map
                   (fun (pattern, body) ->
                     `Assoc
                       [
                         ("pattern", `String pattern); ("body", encode_node body);
                       ])
                   cases) );
            ("default", Option.fold ~none:`Null ~some:encode_node default);
          ]
    | Ir.For_each { variable; items; body } ->
        `Assoc
          [
            ("type", `String "foreach");
            ("variable", `String variable);
            ("items", `List (List.map (fun value -> `String value) items));
            ("body", encode_node body);
          ]
    | Ir.Try_finally { body; finalizer } ->
        `Assoc
          [
            ("type", `String "try_finally");
            ("body", encode_node body);
            ("finalizer", encode_node finalizer);
          ]
    | Ir.Task_call { task; arguments } ->
        `Assoc
          [
            ("type", `String "task_call");
            ("task", `String task);
            ("arguments", encode_pairs arguments);
          ]
    | Ir.Set_variable assignment ->
        `Assoc
          [
            ("type", `String "set_variable");
            ("name", `String assignment.name);
            ("value_type", encode_value_type assignment.value_type);
            ("value", `String assignment.value);
          ]
    | Ir.Capture_stdout capture ->
        `Assoc
          [
            ("type", `String "capture_stdout");
            ("name", `String capture.name);
            ("value_type", encode_value_type capture.value_type);
            ("body", encode_node capture.body);
          ]
    | Ir.File_read path ->
        `Assoc [ ("type", `String "file_read"); ("path", `String path) ]
    | Ir.File_write write ->
        `Assoc
          [
            ("type", `String "file_write");
            ("path", `String write.path);
            ("contents", `String write.contents);
            ("append", `Bool write.append);
          ]
    | Ir.File_remove path ->
        `Assoc [ ("type", `String "file_remove"); ("path", `String path) ]
    | Ir.Network_request request ->
        `Assoc
          [
            ("type", `String "network_request");
            ("method", `String request.method_);
            ("uri", `String request.uri);
          ]
    | Ir.Opaque_capsule capsule ->
        `Assoc
          [
            ("type", `String "opaque_capsule");
            ("interpreter", `String capsule.interpreter);
            ("source", `String capsule.source);
            ("reason", `String capsule.reason);
            ( "path",
              Option.fold ~none:`Null
                ~some:(fun value -> `String value)
                capsule.path );
          ]
  in
  `Assoc
    [
      ("id", `String node.id);
      ("operation", operation);
      ("guarantee", encode_guarantee node.guarantee);
      ("source", Option.fold ~none:`Null ~some:encode_span node.source);
    ]

let rec decode_node context json =
  let* fields = as_object context json in
  let* id_json = required "id" fields in
  let* id = as_string (context ^ ".id") id_json in
  let* guarantee_json = required "guarantee" fields in
  let* guarantee = decode_guarantee (context ^ ".guarantee") guarantee_json in
  let* source =
    match optional "source" fields with
    | None | Some `Null -> Ok None
    | Some source_json ->
        let* source = decode_span (context ^ ".source") source_json in
        Ok (Some source)
  in
  let* operation_json = required "operation" fields in
  let* operation_fields = as_object (context ^ ".operation") operation_json in
  let* type_json = required "type" operation_fields in
  let* operation_type = as_string (context ^ ".operation.type") type_json in
  let node_list name constructor =
    let* nodes_json = required "nodes" operation_fields in
    let* nodes =
      decode_list (context ^ ".operation.nodes") decode_node nodes_json
    in
    Ok (constructor nodes)
  in
  let* operation =
    match operation_type with
    | "exec" ->
        let* argv_json = required "argv" operation_fields in
        let* argv =
          decode_string_list (context ^ ".operation.argv") argv_json
        in
        let* environment =
          match optional "environment" operation_fields with
          | None -> Ok []
          | Some value ->
              decode_pairs (context ^ ".operation.environment") value
        in
        let* working_directory =
          match optional "working_directory" operation_fields with
          | None | Some `Null -> Ok None
          | Some value ->
              let* value =
                as_string (context ^ ".operation.working_directory") value
              in
              Ok (Some value)
        in
        Ok (Ir.Exec Ir.{ argv; environment; working_directory })
    | "pipeline" -> node_list "nodes" (fun nodes -> Ir.Pipeline nodes)
    | "sequence" -> node_list "nodes" (fun nodes -> Ir.Sequence nodes)
    | "parallel" -> node_list "nodes" (fun nodes -> Ir.Parallel nodes)
    | "condition" ->
        let* predicate_json = required "predicate" operation_fields in
        let* predicate =
          decode_node (context ^ ".operation.predicate") predicate_json
        in
        let* if_true_json = required "if_true" operation_fields in
        let* if_true =
          decode_node (context ^ ".operation.if_true") if_true_json
        in
        let* if_false =
          match optional "if_false" operation_fields with
          | None | Some `Null -> Ok None
          | Some value ->
              let* node = decode_node (context ^ ".operation.if_false") value in
              Ok (Some node)
        in
        Ok (Ir.Condition { predicate; if_true; if_false })
    | "match" ->
        let* value_json = required "value" operation_fields in
        let* value = as_string (context ^ ".operation.value") value_json in
        let decode_case case_context case_json =
          let* case_fields = as_object case_context case_json in
          let* pattern_json = required "pattern" case_fields in
          let* pattern = as_string (case_context ^ ".pattern") pattern_json in
          let* body_json = required "body" case_fields in
          let* body = decode_node (case_context ^ ".body") body_json in
          Ok (pattern, body)
        in
        let* cases_json = required "cases" operation_fields in
        let* cases =
          decode_list (context ^ ".operation.cases") decode_case cases_json
        in
        let* default =
          match optional "default" operation_fields with
          | None | Some `Null -> Ok None
          | Some value ->
              let* node = decode_node (context ^ ".operation.default") value in
              Ok (Some node)
        in
        Ok (Ir.Match { value; cases; default })
    | "foreach" ->
        let* variable_json = required "variable" operation_fields in
        let* variable =
          as_string (context ^ ".operation.variable") variable_json
        in
        let* items_json = required "items" operation_fields in
        let* items =
          decode_string_list (context ^ ".operation.items") items_json
        in
        let* body_json = required "body" operation_fields in
        let* body = decode_node (context ^ ".operation.body") body_json in
        Ok (Ir.For_each { variable; items; body })
    | "try_finally" ->
        let* body_json = required "body" operation_fields in
        let* body = decode_node (context ^ ".operation.body") body_json in
        let* finalizer_json = required "finalizer" operation_fields in
        let* finalizer =
          decode_node (context ^ ".operation.finalizer") finalizer_json
        in
        Ok (Ir.Try_finally { body; finalizer })
    | "task_call" ->
        let* task_json = required "task" operation_fields in
        let* task = as_string (context ^ ".operation.task") task_json in
        let* arguments =
          match optional "arguments" operation_fields with
          | None -> Ok []
          | Some value -> decode_pairs (context ^ ".operation.arguments") value
        in
        Ok (Ir.Task_call { task; arguments })
    | "set_variable" ->
        let* name_json = required "name" operation_fields in
        let* name = as_string (context ^ ".operation.name") name_json in
        let* value_type_json = required "value_type" operation_fields in
        let* value_type =
          decode_value_type (context ^ ".operation.value_type") value_type_json
        in
        let* value_json = required "value" operation_fields in
        let* value = as_string (context ^ ".operation.value") value_json in
        Ok (Ir.Set_variable { name; value_type; value })
    | "capture_stdout" ->
        let* name_json = required "name" operation_fields in
        let* name = as_string (context ^ ".operation.name") name_json in
        let* value_type_json = required "value_type" operation_fields in
        let* value_type =
          decode_value_type (context ^ ".operation.value_type") value_type_json
        in
        let* body_json = required "body" operation_fields in
        let* body = decode_node (context ^ ".operation.body") body_json in
        Ok (Ir.Capture_stdout { name; value_type; body })
    | "file_read" ->
        let* path_json = required "path" operation_fields in
        let* path = as_string (context ^ ".operation.path") path_json in
        Ok (Ir.File_read path)
    | "file_write" ->
        let* path_json = required "path" operation_fields in
        let* path = as_string (context ^ ".operation.path") path_json in
        let* contents_json = required "contents" operation_fields in
        let* contents =
          as_string (context ^ ".operation.contents") contents_json
        in
        let* append =
          match optional "append" operation_fields with
          | None -> Ok false
          | Some value -> as_bool (context ^ ".operation.append") value
        in
        Ok (Ir.File_write { path; contents; append })
    | "file_remove" ->
        let* path_json = required "path" operation_fields in
        let* path = as_string (context ^ ".operation.path") path_json in
        Ok (Ir.File_remove path)
    | "network_request" ->
        let* method_json = required "method" operation_fields in
        let* method_ = as_string (context ^ ".operation.method") method_json in
        let* uri_json = required "uri" operation_fields in
        let* uri = as_string (context ^ ".operation.uri") uri_json in
        Ok (Ir.Network_request { method_; uri })
    | "opaque_capsule" ->
        let* interpreter_json = required "interpreter" operation_fields in
        let* interpreter =
          as_string (context ^ ".operation.interpreter") interpreter_json
        in
        let* source_json = required "source" operation_fields in
        let* capsule_source =
          as_string (context ^ ".operation.source") source_json
        in
        let* reason_json = required "reason" operation_fields in
        let* reason = as_string (context ^ ".operation.reason") reason_json in
        let* path =
          match optional "path" operation_fields with
          | None | Some `Null -> Ok None
          | Some value ->
              let* value = as_string (context ^ ".operation.path") value in
              Ok (Some value)
        in
        Ok
          (Ir.Opaque_capsule
             { interpreter; source = capsule_source; reason; path })
    | other -> errorf "%s has unknown operation type %S" context other
  in
  Ok Ir.{ id; operation; guarantee; source }

let encode_binding (binding : Ir.binding) =
  `Assoc
    [
      ("name", `String binding.name);
      ("type", encode_value_type binding.value_type);
    ]

let decode_binding context json =
  let* fields = as_object context json in
  let* name_json = required "name" fields in
  let* name = as_string (context ^ ".name") name_json in
  let* type_json = required "type" fields in
  let* value_type = decode_value_type (context ^ ".type") type_json in
  Ok Ir.{ name; value_type }

let encode_invocation_style = function Ir.Powershell -> `String "powershell"

let decode_invocation_style context json =
  let* value = as_string context json in
  match value with
  | "powershell" -> Ok Ir.Powershell
  | other -> errorf "%s has unknown invocation style %S" context other

let encode_invocation_validation = function
  | Ir.Allow_empty_string -> `Assoc [ ("kind", `String "allow_empty_string") ]
  | Ir.Not_null_or_empty -> `Assoc [ ("kind", `String "not_null_or_empty") ]
  | Ir.String_set { values; ignore_case } ->
      `Assoc
        [
          ("kind", `String "string_set");
          ("values", `List (List.map (fun value -> `String value) values));
          ("ignore_case", `Bool ignore_case);
        ]
  | Ir.Int_range { minimum; maximum } ->
      `Assoc
        [
          ("kind", `String "int_range");
          ("minimum", `Int minimum);
          ("maximum", `Int maximum);
        ]

let decode_invocation_validation context json =
  let* fields = as_object context json in
  let* kind_json = required "kind" fields in
  let* kind = as_string (context ^ ".kind") kind_json in
  match kind with
  | "allow_empty_string" -> Ok Ir.Allow_empty_string
  | "not_null_or_empty" -> Ok Ir.Not_null_or_empty
  | "string_set" ->
      let* values_json = required "values" fields in
      let* values = decode_string_list (context ^ ".values") values_json in
      let* ignore_case =
        match optional "ignore_case" fields with
        | None -> Ok true
        | Some value -> as_bool (context ^ ".ignore_case") value
      in
      Ok (Ir.String_set { values; ignore_case })
  | "int_range" ->
      let* minimum_json = required "minimum" fields in
      let* maximum_json = required "maximum" fields in
      let* minimum = as_int (context ^ ".minimum") minimum_json in
      let* maximum = as_int (context ^ ".maximum") maximum_json in
      Ok (Ir.Int_range { minimum; maximum })
  | other -> errorf "%s has unknown invocation validation %S" context other

let encode_invocation_parameter (parameter : Ir.invocation_parameter) =
  `Assoc
    [
      ("input", `String parameter.input);
      ( "position",
        Option.fold ~none:`Null
          ~some:(fun value -> `Int value)
          parameter.position );
      ("required", `Bool parameter.required);
      ("switch", `Bool parameter.is_switch);
      ( "default",
        Option.fold ~none:`Null
          ~some:(fun value -> `String value)
          parameter.default );
      ( "validations",
        `List (List.map encode_invocation_validation parameter.validations) );
    ]

let decode_invocation_parameter context json =
  let* fields = as_object context json in
  let* input_json = required "input" fields in
  let* input = as_string (context ^ ".input") input_json in
  let* position =
    match optional "position" fields with
    | None | Some `Null -> Ok None
    | Some value ->
        Result.map
          (fun position -> Some position)
          (as_int (context ^ ".position") value)
  in
  let* required_value =
    match optional "required" fields with
    | None -> Ok false
    | Some value -> as_bool (context ^ ".required") value
  in
  let* is_switch =
    match optional "switch" fields with
    | None -> Ok false
    | Some value -> as_bool (context ^ ".switch") value
  in
  let* default =
    match optional "default" fields with
    | None | Some `Null -> Ok None
    | Some value ->
        Result.map
          (fun default -> Some default)
          (as_string (context ^ ".default") value)
  in
  let* validations =
    match optional "validations" fields with
    | None -> Ok []
    | Some value ->
        decode_list (context ^ ".validations") decode_invocation_validation
          value
  in
  Ok
    Ir.
      {
        input;
        position;
        required = required_value;
        is_switch;
        default;
        validations;
      }

let encode_invocation (invocation : Ir.invocation) =
  `Assoc
    [
      ("style", encode_invocation_style invocation.style);
      ("accepts_common_parameters", `Bool invocation.accepts_common_parameters);
      ( "parameters",
        `List (List.map encode_invocation_parameter invocation.parameters) );
    ]

let decode_invocation context json =
  let* fields = as_object context json in
  let* style_json = required "style" fields in
  let* style = decode_invocation_style (context ^ ".style") style_json in
  let* accepts_common_parameters =
    match optional "accepts_common_parameters" fields with
    | None -> Ok false
    | Some value -> as_bool (context ^ ".accepts_common_parameters") value
  in
  let* parameters_json = required "parameters" fields in
  let* parameters =
    decode_list (context ^ ".parameters") decode_invocation_parameter
      parameters_json
  in
  Ok Ir.{ style; accepts_common_parameters; parameters }

let encode_task (task : Ir.task) =
  `Assoc
    [
      ("name", `String task.name);
      ("inputs", `List (List.map encode_binding task.inputs));
      ("outputs", `List (List.map encode_binding task.outputs));
      ( "environment",
        `List (List.map (fun value -> `String value) task.environment) );
      ("secrets", `List (List.map (fun value -> `String value) task.secrets));
      ( "platform_capabilities",
        `List (List.map (fun value -> `String value) task.platform_capabilities)
      );
      ("cacheable", `Bool task.cacheable);
      ( "invocation",
        Option.fold ~none:`Null ~some:encode_invocation task.invocation );
      ("body", encode_node task.body);
    ]

let decode_task context json =
  let* fields = as_object context json in
  let* name_json = required "name" fields in
  let* name = as_string (context ^ ".name") name_json in
  let list_field field decoder default =
    match optional field fields with
    | None -> Ok default
    | Some value -> decode_list (context ^ "." ^ field) decoder value
  in
  let* inputs = list_field "inputs" decode_binding [] in
  let* outputs = list_field "outputs" decode_binding [] in
  let* environment =
    match optional "environment" fields with
    | None -> Ok []
    | Some value -> decode_string_list (context ^ ".environment") value
  in
  let* secrets =
    match optional "secrets" fields with
    | None -> Ok []
    | Some value -> decode_string_list (context ^ ".secrets") value
  in
  let* platform_capabilities =
    match optional "platform_capabilities" fields with
    | None -> Ok []
    | Some value ->
        decode_string_list (context ^ ".platform_capabilities") value
  in
  let* cacheable =
    match optional "cacheable" fields with
    | None -> Ok false
    | Some value -> as_bool (context ^ ".cacheable") value
  in
  let* invocation =
    match optional "invocation" fields with
    | None | Some `Null -> Ok None
    | Some value ->
        Result.map
          (fun invocation -> Some invocation)
          (decode_invocation (context ^ ".invocation") value)
  in
  let* body_json = required "body" fields in
  let* body = decode_node (context ^ ".body") body_json in
  Ok
    Ir.
      {
        name;
        inputs;
        outputs;
        environment;
        secrets;
        platform_capabilities;
        cacheable;
        invocation;
        body;
      }

let encode_yojson (plan : Ir.plan) =
  `Assoc
    [
      ("schema_version", `Int plan.schema_version);
      ("generator", `String plan.generator);
      ("entrypoint", `String plan.entrypoint);
      ("tasks", `List (List.map encode_task plan.tasks));
    ]

let decode_v0 fields =
  let* entrypoint =
    match optional "entrypoint" fields with
    | None -> Ok "main"
    | Some value -> as_string "entrypoint" value
  in
  let* commands_json = required "commands" fields in
  let* commands =
    decode_list "commands"
      (fun context value -> decode_string_list context value)
      commands_json
  in
  let nodes =
    List.mapi
      (fun index argv ->
        Ir.node
          ~id:(Printf.sprintf "legacy-%d" (index + 1))
          ~guarantee:(Ir.Formal { basis = "schema-v0-command-migration" })
          (Ir.Exec (Ir.exec argv)))
      commands
  in
  let body =
    match nodes with
    | [ node ] -> node
    | _ ->
        Ir.node ~id:"legacy-sequence"
          ~guarantee:(Ir.Formal { basis = "schema-v0-sequence-migration" })
          (Ir.Sequence nodes)
  in
  Ok (Ir.plan ~entrypoint [ Ir.task ~name:entrypoint ~body () ])

let decode_current fields =
  let* generator =
    match optional "generator" fields with
    | None -> Ok "unknown"
    | Some value -> as_string "generator" value
  in
  let* entrypoint_json = required "entrypoint" fields in
  let* entrypoint = as_string "entrypoint" entrypoint_json in
  let* tasks_json = required "tasks" fields in
  let* tasks = decode_list "tasks" decode_task tasks_json in
  let plan =
    Ir.
      {
        schema_version = Ir.current_schema_version;
        generator;
        entrypoint;
        tasks;
      }
  in
  match Ir.validate_plan plan with
  | Ok () -> Ok plan
  | Error errors -> Error errors

let decode_yojson json =
  let* fields = as_object "plan" json in
  match (optional "schema_version" fields, optional "version" fields) with
  | Some value, _ ->
      let* version = as_int "schema_version" value in
      if version = 1 || version = 2 || version = Ir.current_schema_version then
        decode_current fields
      else errorf "unsupported schema_version: %d" version
  | None, Some value ->
      let* version = as_int "version" value in
      if version = 0 then decode_v0 fields
      else errorf "unsupported legacy version: %d" version
  | None, None -> error "missing required field: schema_version"

let encode_string plan =
  Yojson.Safe.pretty_to_string (encode_yojson plan) ^ "\n"

let decode_string input =
  try decode_yojson (Yojson.Safe.from_string input)
  with Yojson.Json_error message -> error ("invalid JSON: " ^ message)
