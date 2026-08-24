let current_schema_version = 3

type value_type =
  | Bytes
  | Text
  | Bool
  | Int
  | Path
  | List of value_type
  | Record of (string * value_type) list
  | Secret of value_type
  | Byte_stream
  | Object_stream of value_type

type guarantee =
  | Formal of { basis : string }
  | Exhaustive of { scenarios : string list }
  | Differential of { scenarios : string list; observation_digest : string }
  | Residual of { reason : string }

type source_span = {
  file : string;
  start_line : int;
  start_column : int;
  end_line : int;
  end_column : int;
  start_byte : int;
  end_byte : int;
}

type command = {
  argv : string list;
  environment : (string * string) list;
  working_directory : string option;
}

type opaque_capsule = {
  interpreter : string;
  source : string;
  reason : string;
  path : string option;
}

type file_write = { path : string; contents : string; append : bool }
type network_request = { method_ : string; uri : string }

type variable_assignment = {
  name : string;
  value_type : value_type;
  value : string;
}

type node = {
  id : string;
  operation : operation;
  guarantee : guarantee;
  source : source_span option;
}

and operation =
  | Exec of command
  | Pipeline of node list
  | Sequence of node list
  | Parallel of node list
  | Condition of { predicate : node; if_true : node; if_false : node option }
  | Match of {
      value : string;
      cases : (string * node) list;
      default : node option;
    }
  | For_each of { variable : string; items : string list; body : node }
  | Try_finally of { body : node; finalizer : node }
  | Task_call of { task : string; arguments : (string * string) list }
  | Set_variable of variable_assignment
  | Capture_stdout of { name : string; value_type : value_type; body : node }
  | File_read of string
  | File_write of file_write
  | File_remove of string
  | Network_request of network_request
  | Opaque_capsule of opaque_capsule

type binding = { name : string; value_type : value_type }
type invocation_style = Powershell

type invocation_validation =
  | Allow_empty_string
  | Not_null_or_empty
  | String_set of { values : string list; ignore_case : bool }
  | Int_range of { minimum : int; maximum : int }

type invocation_parameter = {
  input : string;
  position : int option;
  required : bool;
  is_switch : bool;
  default : string option;
  validations : invocation_validation list;
}

type invocation = {
  style : invocation_style;
  accepts_common_parameters : bool;
  parameters : invocation_parameter list;
}

type task = {
  name : string;
  inputs : binding list;
  outputs : binding list;
  environment : string list;
  secrets : string list;
  platform_capabilities : string list;
  cacheable : bool;
  invocation : invocation option;
  body : node;
}

type plan = {
  schema_version : int;
  generator : string;
  entrypoint : string;
  tasks : task list;
}

let exec ?(environment = []) ?working_directory argv =
  { argv; environment; working_directory }

let opaque ~interpreter ~source ~reason =
  { interpreter; source; reason; path = None }

let opaque_file ~path ~interpreter ~source ~reason =
  { interpreter; source; reason; path = Some path }

let node ?source ~id ~guarantee operation = { id; operation; guarantee; source }

let task ?(inputs = []) ?(outputs = []) ?(environment = []) ?(secrets = [])
    ?(platform_capabilities = []) ?(cacheable = false) ?invocation ~name ~body
    () =
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

let plan ?(generator = "deshell/0.1.0") ~entrypoint tasks =
  { schema_version = current_schema_version; generator; entrypoint; tasks }

let equal_plan left right = left = right

let non_empty label value errors =
  if String.trim value = "" then (label ^ " must not be empty") :: errors
  else errors

let duplicate_values label values errors =
  let seen = Hashtbl.create (List.length values) in
  List.fold_left
    (fun errors value ->
      if Hashtbl.mem seen value then
        ("duplicate " ^ label ^ ": " ^ value) :: errors
      else begin
        Hashtbl.add seen value ();
        errors
      end)
    errors values

let validate_named_values label values errors =
  let errors = duplicate_values label values errors in
  List.fold_left
    (fun errors value -> non_empty label value errors)
    errors values

let valid_variable_name name =
  name <> ""
  &&
  match name.[0] with
  | 'A' .. 'Z' | 'a' .. 'z' | '_' ->
      String.for_all
        (function
          | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> true | _ -> false)
        name
  | _ -> false

let rec scalar_runtime_value_type = function
  | Bytes | Text | Bool | Int | Path -> true
  | Secret value_type -> scalar_runtime_value_type value_type
  | List _ | Record _ | Byte_stream | Object_stream _ -> false

let rec contains_state_mutation node =
  match node.operation with
  | Set_variable _ | Capture_stdout _ -> true
  | Exec _ | Task_call _ | File_read _ | File_write _ | File_remove _
  | Network_request _ | Opaque_capsule _ ->
      false
  | Pipeline nodes | Sequence nodes | Parallel nodes ->
      List.exists contains_state_mutation nodes
  | Condition { predicate; if_true; if_false } ->
      contains_state_mutation predicate
      || contains_state_mutation if_true
      || Option.exists contains_state_mutation if_false
  | Match { cases; default; _ } ->
      List.exists (fun (_, branch) -> contains_state_mutation branch) cases
      || Option.exists contains_state_mutation default
  | For_each { body; _ } -> contains_state_mutation body
  | Try_finally { body; finalizer } ->
      contains_state_mutation body || contains_state_mutation finalizer

let validate_span span errors =
  let errors = non_empty "source span file" span.file errors in
  let position_reversed =
    span.end_line < span.start_line
    || (span.end_line = span.start_line && span.end_column < span.start_column)
  in
  if
    span.start_line < 1 || span.end_line < 1 || span.start_column < 0
    || span.end_column < 0 || span.start_byte < 0
    || span.end_byte < span.start_byte
    || position_reversed
  then "source span is not well formed" :: errors
  else errors

let rec validate_node task_table seen node errors =
  let errors = non_empty "node id" node.id errors in
  let errors =
    if Hashtbl.mem seen node.id then ("duplicate node id: " ^ node.id) :: errors
    else begin
      Hashtbl.add seen node.id ();
      errors
    end
  in
  let errors =
    match node.source with
    | None -> errors
    | Some span -> validate_span span errors
  in
  let errors =
    match node.guarantee with
    | Formal { basis } -> non_empty "formal basis" basis errors
    | Exhaustive { scenarios } ->
        let errors =
          if scenarios = [] then
            "exhaustive guarantee requires scenarios" :: errors
          else errors
        in
        validate_named_values "guarantee scenario" scenarios errors
    | Differential { scenarios; observation_digest } ->
        let errors =
          if scenarios = [] then
            "differential guarantee requires scenarios" :: errors
          else errors
        in
        let errors =
          validate_named_values "guarantee scenario" scenarios errors
        in
        non_empty "observation digest" observation_digest errors
    | Residual { reason } -> non_empty "residual reason" reason errors
  in
  let validate_children children errors =
    List.fold_left
      (fun current child -> validate_node task_table seen child current)
      errors children
  in
  match node.operation with
  | Exec command ->
      let errors =
        match command.argv with
        | [] -> "Exec argv must not be empty" :: errors
        | executable :: _ -> non_empty "Exec executable" executable errors
      in
      let environment_names = List.map fst command.environment in
      let errors =
        duplicate_values "Exec environment" environment_names errors
      in
      let errors =
        List.fold_left
          (fun errors name -> non_empty "Exec environment name" name errors)
          errors environment_names
      in
      Option.fold ~none:errors
        ~some:(fun directory ->
          non_empty "Exec working directory" directory errors)
        command.working_directory
  | Pipeline nodes ->
      let errors =
        if nodes = [] then "Pipeline must contain at least one node" :: errors
        else errors
      in
      let errors =
        if List.exists contains_state_mutation nodes then
          "pipeline state mutation is undefined; use an explicit task boundary"
          :: errors
        else errors
      in
      validate_children nodes errors
  | Sequence nodes -> validate_children nodes errors
  | Parallel nodes ->
      let errors =
        if List.exists contains_state_mutation nodes then
          "parallel state mutation is nondeterministic; use isolated task \
           inputs" :: errors
        else errors
      in
      validate_children nodes errors
  | Condition { predicate; if_true; if_false } ->
      let errors = validate_node task_table seen predicate errors in
      let errors = validate_node task_table seen if_true errors in
      Option.fold ~none:errors
        ~some:(fun value -> validate_node task_table seen value errors)
        if_false
  | Match { cases; default; _ } ->
      let labels = List.map fst cases in
      let errors = duplicate_values "match case" labels errors in
      let errors =
        List.fold_left
          (fun errors label -> non_empty "match case label" label errors)
          errors labels
      in
      let errors =
        List.fold_left
          (fun current (_, branch) ->
            validate_node task_table seen branch current)
          errors cases
      in
      Option.fold ~none:errors
        ~some:(fun value -> validate_node task_table seen value errors)
        default
  | For_each { variable; body; _ } ->
      validate_node task_table seen body
        (non_empty "foreach variable" variable errors)
  | Try_finally { body; finalizer } ->
      let errors =
        if contains_state_mutation body || contains_state_mutation finalizer
        then
          "try/finally state mutation is undefined across failure paths"
          :: errors
        else errors
      in
      validate_node task_table seen finalizer
        (validate_node task_table seen body errors)
  | Task_call { task; arguments } ->
      let errors = non_empty "task call target" task errors in
      begin match Hashtbl.find_opt task_table task with
      | None when task <> "" -> ("task not found: " ^ task) :: errors
      | None -> errors
      | Some target ->
          let argument_names = List.map fst arguments in
          let errors = duplicate_values "argument" argument_names errors in
          let expected_names =
            List.map (fun (binding : binding) -> binding.name) target.inputs
          in
          let errors =
            List.fold_left
              (fun errors name ->
                if List.mem name expected_names then errors
                else
                  Printf.sprintf "unknown argument %s for task %s" name task
                  :: errors)
              errors argument_names
          in
          List.fold_left
            (fun errors name ->
              if List.mem name argument_names then errors
              else
                Printf.sprintf "missing argument %s for task %s" name task
                :: errors)
            errors expected_names
      end
  | Set_variable assignment ->
      let errors =
        if valid_variable_name assignment.name then errors
        else
          ("runtime variable name is not valid: " ^ assignment.name) :: errors
      in
      if scalar_runtime_value_type assignment.value_type then errors
      else
        ("runtime variable " ^ assignment.name ^ " must use a scalar value type")
        :: errors
  | Capture_stdout capture ->
      let errors =
        if valid_variable_name capture.name then errors
        else ("runtime variable name is not valid: " ^ capture.name) :: errors
      in
      let errors =
        if capture.value_type = Text then errors
        else
          ("stdout capture variable " ^ capture.name ^ " must use text type")
          :: errors
      in
      validate_node task_table seen capture.body errors
  | File_read path | File_remove path -> non_empty "file path" path errors
  | File_write write -> non_empty "file path" write.path errors
  | Network_request request ->
      non_empty "network URI" request.uri
        (non_empty "network method" request.method_ errors)
  | Opaque_capsule capsule ->
      let errors = non_empty "capsule interpreter" capsule.interpreter errors in
      let errors = non_empty "capsule source" capsule.source errors in
      let errors = non_empty "residual reason" capsule.reason errors in
      let errors =
        match node.guarantee with
        | Residual { reason } when reason = capsule.reason -> errors
        | Residual _ -> "capsule residual reason must match guarantee" :: errors
        | Formal _ | Exhaustive _ | Differential _ ->
            "opaque capsule must use a residual guarantee" :: errors
      in
      begin match capsule.path with
      | None -> errors
      | Some path ->
          let normalized =
            String.map
              (fun character -> if character = '\\' then '/' else character)
              path
          in
          if
            path <> "" && Filename.is_relative path
            && normalized |> String.split_on_char '/'
               |> List.for_all (fun part ->
                   part <> "" && part <> "." && part <> "..")
          then errors
          else
            ("capsule path must be safely project-relative: " ^ path) :: errors
      end

let rec unsecret_value_type = function
  | Secret inner -> unsecret_value_type inner
  | value_type -> value_type

let invocation_validation_name = function
  | Allow_empty_string -> "allow_empty_string"
  | Not_null_or_empty -> "not_null_or_empty"
  | String_set _ -> "string_set"
  | Int_range _ -> "int_range"

let powershell_common_parameter_names =
  [
    "verbose";
    "vb";
    "debug";
    "db";
    "erroraction";
    "ea";
    "warningaction";
    "wa";
    "informationaction";
    "infa";
    "progressaction";
    "proga";
    "errorvariable";
    "ev";
    "warningvariable";
    "wv";
    "informationvariable";
    "iv";
    "outvariable";
    "ov";
    "outbuffer";
    "ob";
    "pipelinevariable";
    "pv";
  ]

let normalize_powershell_int32 value =
  let value = String.trim value in
  let error () = Error "must be a PowerShell Int32" in
  let int32_min = -2_147_483_648L in
  let int32_max = 2_147_483_647L in
  let uint32_modulus = 4_294_967_296L in
  let decimal_digit = function '0' .. '9' -> true | _ -> false in
  let digit_value radix = function
    | '0' .. '9' as character ->
        let value = Char.code character - Char.code '0' in
        if value < radix then Some value else None
    | ('a' .. 'f' | 'A' .. 'F') as character when radix = 16 ->
        Some (10 + Char.code (Char.lowercase_ascii character) - Char.code 'a')
    | _ -> None
  in
  let parse_unsigned radix digits =
    if digits = "" then None
    else
      let rec loop index accumulator =
        if index = String.length digits then Some accumulator
        else
          match digit_value radix digits.[index] with
          | None -> None
          | Some digit ->
              let radix = Int64.of_int radix in
              let digit = Int64.of_int digit in
              if
                Int64.compare accumulator
                  (Int64.div (Int64.sub Int64.max_int digit) radix)
                > 0
              then None
              else
                loop (index + 1) (Int64.add (Int64.mul accumulator radix) digit)
      in
      loop 0 0L
  in
  let signed_prefix value =
    if value = "" then (1, value)
    else
      match value.[0] with
      | '+' -> (1, String.sub value 1 (String.length value - 1))
      | '-' -> (-1, String.sub value 1 (String.length value - 1))
      | _ -> (1, value)
  in
  let sign, unsigned = signed_prefix value in
  let base_literal =
    if
      String.length unsigned > 2
      && unsigned.[0] = '0'
      && (unsigned.[1] = 'x' || unsigned.[1] = 'X')
    then Some (16, String.sub unsigned 2 (String.length unsigned - 2))
    else if
      String.length unsigned > 2
      && unsigned.[0] = '0'
      && (unsigned.[1] = 'b' || unsigned.[1] = 'B')
    then Some (2, String.sub unsigned 2 (String.length unsigned - 2))
    else None
  in
  match base_literal with
  | Some (radix, digits) ->
      begin match parse_unsigned radix digits with
      | Some magnitude when Int64.compare magnitude 0xffff_ffffL <= 0 ->
          let signed =
            if Int64.compare magnitude int32_max <= 0 then magnitude
            else Int64.sub magnitude uint32_modulus
          in
          let signed = if sign < 0 then Int64.neg signed else signed in
          if
            Int64.compare signed int32_min < 0
            || Int64.compare signed int32_max > 0
          then error ()
          else Ok (Int64.to_string signed)
      | Some _ | None -> error ()
      end
  | None -> (
      let valid_decimal =
        let length = String.length value in
        let rec digits index =
          if index < length && decimal_digit value.[index] then
            digits (index + 1)
          else index
        in
        let start =
          if length > 0 && (value.[0] = '+' || value.[0] = '-') then 1 else 0
        in
        let integer_end = digits start in
        let has_integer = integer_end > start in
        let fraction_end, has_fraction =
          if integer_end < length && value.[integer_end] = '.' then
            let finish = digits (integer_end + 1) in
            (finish, finish > integer_end + 1)
          else (integer_end, false)
        in
        let mantissa = has_integer || has_fraction in
        let exponent_end =
          if
            fraction_end < length
            && (value.[fraction_end] = 'e' || value.[fraction_end] = 'E')
          then
            let exponent_start =
              if
                fraction_end + 1 < length
                && (value.[fraction_end + 1] = '+'
                   || value.[fraction_end + 1] = '-')
              then fraction_end + 2
              else fraction_end + 1
            in
            let finish = digits exponent_start in
            if finish = exponent_start then -1 else finish
          else fraction_end
        in
        mantissa && exponent_end = length
      in
      if not valid_decimal then error ()
      else
        match float_of_string_opt value with
        | None -> error ()
        | Some number
          when Float.is_nan number || Float.is_infinite number
               || number < -2_147_483_648.5 || number > 2_147_483_647.5 ->
            error ()
        | Some number ->
            let floor = Float.floor number in
            let fraction = number -. floor in
            let rounded =
              if fraction < 0.5 then floor
              else if fraction > 0.5 then floor +. 1.
              else
                let floor_int = Int64.of_float floor in
                if Int64.rem floor_int 2L = 0L then floor else floor +. 1.
            in
            let normalized = Int64.of_float rounded in
            if
              Int64.compare normalized int32_min < 0
              || Int64.compare normalized int32_max > 0
            then error ()
            else Ok (Int64.to_string normalized))

let validate_invocation_validation task_name parameter input validation errors =
  let type_error expected errors =
    Printf.sprintf
      "task %s invocation validation %s for %s requires %s input type" task_name
      (invocation_validation_name validation)
      parameter.input expected
    :: errors
  in
  let input_type =
    Option.map
      (fun (binding : binding) -> unsecret_value_type binding.value_type)
      input
  in
  match validation with
  | Allow_empty_string | Not_null_or_empty ->
      begin match input_type with
      | Some Text -> errors
      | Some _ | None -> type_error "text" errors
      end
  | String_set { values; ignore_case } ->
      let normalized =
        if ignore_case then List.map String.lowercase_ascii values else values
      in
      let errors =
        if values = [] then
          Printf.sprintf
            "task %s invocation string set for %s must not be empty" task_name
            parameter.input
          :: errors
        else errors
      in
      let errors =
        duplicate_values
          ("invocation string set value for " ^ parameter.input)
          normalized errors
      in
      begin match input_type with
      | Some Text -> errors
      | Some _ | None -> type_error "text" errors
      end
  | Int_range { minimum; maximum } ->
      let errors =
        if minimum > maximum then
          Printf.sprintf
            "task %s invocation integer range for %s has minimum greater than \
             maximum"
            task_name parameter.input
          :: errors
        else errors
      in
      begin match input_type with
      | Some Int -> errors
      | Some _ | None -> type_error "int" errors
      end

let normalize_invocation_default value_type value =
  let rec normalize = function
    | Text | Path | Bytes -> Ok value
    | Int -> normalize_powershell_int32 value
    | Bool ->
        begin match String.lowercase_ascii value with
        | "true" | "$true" | "1" -> Ok "true"
        | "false" | "$false" | "0" -> Ok "false"
        | _ -> Error "must be a boolean"
        end
    | Secret inner -> normalize inner
    | List _ | Record _ | Byte_stream | Object_stream _ ->
        Error "uses an input type unsupported by PowerShell invocation"
  in
  normalize value_type

let validate_invocation_default task_name parameter input errors =
  match (parameter.default, input) with
  | None, _ | Some _, None -> errors
  | Some default, Some (binding : binding) ->
      let invalid detail errors =
        Printf.sprintf "task %s invocation default for %s %s" task_name
          parameter.input detail
        :: errors
      in
      begin match normalize_invocation_default binding.value_type default with
      | Error detail -> invalid detail errors
      | Ok _ -> errors
      end

let validate_plan plan =
  let errors = ref [] in
  if plan.schema_version <> current_schema_version then
    errors :=
      Printf.sprintf "unsupported schema_version: %d" plan.schema_version
      :: !errors;
  errors := non_empty "entrypoint" plan.entrypoint !errors;
  if plan.tasks = [] then
    errors := "plan must contain at least one task" :: !errors;
  let task_names = Hashtbl.create (List.length plan.tasks) in
  List.iter
    (fun (task : task) ->
      errors := non_empty "task name" task.name !errors;
      if Hashtbl.mem task_names task.name then
        errors := ("duplicate task: " ^ task.name) :: !errors
      else Hashtbl.add task_names task.name task)
    plan.tasks;
  List.iter
    (fun (task : task) ->
      let input_names =
        List.map (fun (binding : binding) -> binding.name) task.inputs
      in
      let output_names =
        List.map (fun (binding : binding) -> binding.name) task.outputs
      in
      errors := duplicate_values "input" input_names !errors;
      errors := duplicate_values "output" output_names !errors;
      errors := duplicate_values "environment entry" task.environment !errors;
      errors := duplicate_values "secret" task.secrets !errors;
      errors :=
        validate_named_values "platform capability" task.platform_capabilities
          !errors;
      List.iter
        (fun name -> errors := non_empty "input name" name !errors)
        input_names;
      List.iter
        (fun name -> errors := non_empty "output name" name !errors)
        output_names;
      List.iter
        (fun name -> errors := non_empty "environment name" name !errors)
        task.environment;
      List.iter
        (fun secret ->
          if List.mem secret task.environment then ()
          else
            match
              List.find_opt
                (fun (binding : binding) -> binding.name = secret)
                task.inputs
            with
            | None ->
                errors :=
                  Printf.sprintf
                    "task %s secret %s is not an input or environment entry"
                    task.name secret
                  :: !errors
            | Some { value_type = Secret _; _ } -> ()
            | Some _ ->
                errors :=
                  Printf.sprintf "task %s secret %s must use a secret type"
                    task.name secret
                  :: !errors)
        task.secrets;
      List.iter
        (fun (binding : binding) ->
          match binding.value_type with
          | Secret _ when not (List.mem binding.name task.secrets) ->
              errors :=
                Printf.sprintf "task %s secret input %s is not declared"
                  task.name binding.name
                :: !errors
          | _ -> ())
        task.inputs;
      begin match task.invocation with
      | None -> ()
      | Some invocation ->
          let powershell_input_names =
            List.map String.lowercase_ascii input_names
          in
          errors :=
            duplicate_values "case-insensitive PowerShell input"
              powershell_input_names !errors;
          let parameter_names =
            List.map
              (fun parameter -> String.lowercase_ascii parameter.input)
              invocation.parameters
          in
          errors :=
            duplicate_values "invocation parameter" parameter_names !errors;
          let positions =
            List.filter_map
              (fun parameter -> Option.map string_of_int parameter.position)
              invocation.parameters
          in
          errors := duplicate_values "invocation position" positions !errors;
          List.iter
            (fun parameter ->
              let input =
                List.find_opt
                  (fun (binding : binding) -> binding.name = parameter.input)
                  task.inputs
              in
              if Option.is_none input then
                errors :=
                  Printf.sprintf
                    "task %s invocation parameter %s refers to an unknown task \
                     input"
                    task.name parameter.input
                  :: !errors;
              if
                invocation.accepts_common_parameters
                && List.mem
                     (String.lowercase_ascii parameter.input)
                     powershell_common_parameter_names
              then
                errors :=
                  Printf.sprintf
                    "task %s invocation parameter %s conflicts with a \
                     PowerShell common parameter"
                    task.name parameter.input
                  :: !errors;
              begin match parameter.position with
              | Some position when position < 0 ->
                  errors :=
                    Printf.sprintf
                      "task %s invocation position for %s must be non-negative"
                      task.name parameter.input
                    :: !errors
              | Some _ | None -> ()
              end;
              let validation_names =
                List.map invocation_validation_name parameter.validations
              in
              errors :=
                duplicate_values
                  ("invocation validation for " ^ parameter.input)
                  validation_names !errors;
              if
                List.mem Allow_empty_string parameter.validations
                && List.mem Not_null_or_empty parameter.validations
              then
                errors :=
                  Printf.sprintf
                    "task %s invocation parameter %s has conflicting \
                     empty-string validations"
                    task.name parameter.input
                  :: !errors;
              List.iter
                (fun validation ->
                  errors :=
                    validate_invocation_validation task.name parameter input
                      validation !errors)
                parameter.validations;
              errors :=
                validate_invocation_default task.name parameter input !errors;
              if parameter.is_switch then
                match input with
                | Some { value_type = Bool; _ } -> ()
                | Some _ | None ->
                    errors :=
                      Printf.sprintf
                        "task %s switch invocation parameter %s must use bool \
                         input type"
                        task.name parameter.input
                      :: !errors)
            invocation.parameters
      end)
    plan.tasks;
  let node_ids = Hashtbl.create 32 in
  List.iter
    (fun task -> errors := validate_node task_names node_ids task.body !errors)
    plan.tasks;
  if plan.entrypoint <> "" && not (Hashtbl.mem task_names plan.entrypoint) then
    errors := ("entrypoint task not found: " ^ plan.entrypoint) :: !errors;
  match List.rev !errors with [] -> Ok () | values -> Error values

let rec fold_nodes f accumulator node =
  let accumulator = f accumulator node in
  match node.operation with
  | Exec _ | Task_call _ | Set_variable _ | File_read _ | File_write _
  | File_remove _ | Network_request _ | Opaque_capsule _ ->
      accumulator
  | Capture_stdout { body; _ } -> fold_nodes f accumulator body
  | Pipeline nodes | Sequence nodes | Parallel nodes ->
      List.fold_left (fold_nodes f) accumulator nodes
  | Condition { predicate; if_true; if_false } ->
      let accumulator = fold_nodes f accumulator predicate in
      let accumulator = fold_nodes f accumulator if_true in
      Option.fold ~none:accumulator
        ~some:(fun branch -> fold_nodes f accumulator branch)
        if_false
  | Match { cases; default; _ } ->
      let accumulator =
        List.fold_left
          (fun state (_, branch) -> fold_nodes f state branch)
          accumulator cases
      in
      Option.fold ~none:accumulator
        ~some:(fun branch -> fold_nodes f accumulator branch)
        default
  | For_each { body; _ } -> fold_nodes f accumulator body
  | Try_finally { body; finalizer } ->
      fold_nodes f (fold_nodes f accumulator body) finalizer
