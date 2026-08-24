type process_request = {
  argv : string list;
  environment : (string * string) list;
  working_directory : string option;
  stdin : string;
}

type process_result = { exit_code : int; stdout : string; stderr : string }

type backend = {
  execute : process_request -> (process_result, string) result;
  read_file : string -> (string, string) result;
  write_file :
    path:string -> contents:string -> append:bool -> (unit, string) result;
  remove_file : string -> (unit, string) result;
  network_request : method_:string -> uri:string -> (string, string) result;
}

type policy = {
  allow_file_read : bool;
  allow_file_write : bool;
  allow_network : bool;
  allow_opaque : bool;
}

type trace_event =
  | Process of string list * int
  | File_read of string
  | File_write of string
  | File_remove of string
  | Network of string * string
  | Capsule of string

type observation = {
  exit_code : int;
  stdout : string;
  stderr : string;
  trace : trace_event list;
}

let ( let* ) result continuation =
  match result with Ok value -> continuation value | Error _ as error -> error

let default_policy =
  {
    allow_file_read = false;
    allow_file_write = false;
    allow_network = false;
    allow_opaque = false;
  }

let permissive_policy =
  {
    allow_file_read = true;
    allow_file_write = true;
    allow_network = true;
    allow_opaque = true;
  }

let empty = { exit_code = 0; stdout = ""; stderr = ""; trace = [] }

let combine ~exit_code left right =
  {
    exit_code;
    stdout = left.stdout ^ right.stdout;
    stderr = left.stderr ^ right.stderr;
    trace = left.trace @ right.trace;
  }

let process_observation ~trace_argv (result : process_result) =
  {
    exit_code = result.exit_code;
    stdout = result.stdout;
    stderr = result.stderr;
    trace = [ Process (trace_argv, result.exit_code) ];
  }

let command_policy_error policy argv =
  let classification = Command_model.classify argv in
  if not classification.known then None
  else if
    List.mem Command_model.Filesystem_write classification.capabilities
    && not policy.allow_file_write
  then
    Some ("file write denied by policy for command: " ^ classification.command)
  else if
    List.mem Command_model.Filesystem_read classification.capabilities
    && not policy.allow_file_read
  then Some ("file read denied by policy for command: " ^ classification.command)
  else if
    List.mem Command_model.Network classification.capabilities
    && not policy.allow_network
  then Some ("network denied by policy for command: " ^ classification.command)
  else None

let capsule_argv capsule arguments =
  let interpreter = String.lowercase_ascii capsule.Ir.interpreter in
  match (interpreter, capsule.path) with
  | "cmd", Some path ->
      let quote value =
        if
          value <> ""
          && String.for_all
               (function
                 | 'A' .. 'Z'
                 | 'a' .. 'z'
                 | '0' .. '9'
                 | '_' | '-' | '.' | '/' | '\\' | ':' ->
                     true
                 | _ -> false)
               value
        then value
        else "\"" ^ String.concat "\"\"" (String.split_on_char '"' value) ^ "\""
      in
      let command =
        "call " ^ String.concat " " (List.map quote (path :: arguments))
      in
      [ "cmd"; "/d"; "/s"; "/c"; command ]
  | "powershell", Some path ->
      [
        (if Sys.win32 then "powershell.exe" else "pwsh");
        "-NoLogo";
        "-NoProfile";
        "-NonInteractive";
        "-File";
        path;
      ]
      @ arguments
  | "pwsh", Some path ->
      [ "pwsh"; "-NoLogo"; "-NoProfile"; "-NonInteractive"; "-File"; path ]
      @ arguments
  | ("nu" | "nushell"), Some path -> [ "nu"; path ] @ arguments
  | interpreter, Some path -> interpreter :: path :: arguments
  | "cmd", None -> [ "cmd"; "/d"; "/s"; "/c"; capsule.source ]
  | ("powershell" | "pwsh"), None ->
      [
        (if interpreter = "powershell" && Sys.win32 then "powershell.exe"
         else "pwsh");
        "-NoLogo";
        "-NoProfile";
        "-NonInteractive";
        "-Command";
        capsule.source;
      ]
      @ arguments
  | ("nu" | "nushell"), None -> [ "nu"; "-c"; capsule.source ] @ arguments
  | interpreter, None ->
      let base = [ interpreter; "-c"; capsule.source ] in
      if arguments = [] then base else base @ ("deshell-capsule" :: arguments)

type context = {
  variables : (string * string) list;
  secret_names : string list;
  script_arguments : string list;
}

let redact_error context message =
  let secrets =
    context.secret_names
    |> List.filter_map (fun name ->
        match List.assoc_opt name context.variables with
        | Some value when value <> "" -> Some (value, "<secret:" ^ name ^ ">")
        | Some _ | None -> None)
    |> List.sort (fun (left, _) (right, _) ->
        Int.compare (String.length right) (String.length left))
  in
  let length = String.length message in
  let matches index value =
    let value_length = String.length value in
    index + value_length <= length
    && String.sub message index value_length = value
  in
  let buffer = Buffer.create length in
  let rec loop index =
    if index >= length then Buffer.contents buffer
    else
      match List.find_opt (fun (value, _) -> matches index value) secrets with
      | Some (value, placeholder) ->
          Buffer.add_string buffer placeholder;
          loop (index + String.length value)
      | None ->
          Buffer.add_char buffer message.[index];
          loop (index + 1)
  in
  loop 0

let redact_backend_error context = Result.map_error (redact_error context)

let add_variable variables (name, value) =
  (name, value) :: List.remove_assoc name variables

let restore_variable variables name previous =
  match previous with
  | None -> List.remove_assoc name variables
  | Some value -> add_variable variables (name, value)

let expand_text context text =
  let length = String.length text in
  let actual = Buffer.create length in
  let redacted = Buffer.create length in
  let rec find_close index =
    if index >= length then None
    else if text.[index] = '}' then Some index
    else find_close (index + 1)
  in
  let rec loop index =
    if index >= length then Ok (Buffer.contents actual, Buffer.contents redacted)
    else if index + 1 < length && text.[index] = '$' && text.[index + 1] = '$'
    then begin
      Buffer.add_char actual '$';
      Buffer.add_char redacted '$';
      loop (index + 2)
    end
    else if index + 1 < length && text.[index] = '$' && text.[index + 1] = '{'
    then
      match find_close (index + 2) with
      | None -> Error ("unterminated variable reference in " ^ text)
      | Some close ->
          let expression = String.sub text (index + 2) (close - index - 2) in
          let parameter =
            match String.index_opt expression ':' with
            | Some separator
              when separator + 1 < String.length expression
                   && expression.[separator + 1] = '-' ->
                Ok
                  ( String.sub expression 0 separator,
                    Some
                      (String.sub expression (separator + 2)
                         (String.length expression - separator - 2)) )
            | Some _ ->
                Error ("unsupported parameter expression: ${" ^ expression ^ "}")
            | None -> Ok (expression, None)
          in
          begin match parameter with
          | Error _ as error -> error
          | Ok (name, default) ->
              let is_positional =
                name <> ""
                && String.for_all
                     (function '0' .. '9' -> true | _ -> false)
                     name
              in
              let valid_named =
                name <> ""
                &&
                match name.[0] with
                | 'A' .. 'Z' | 'a' .. 'z' | '_' ->
                    String.for_all
                      (function
                        | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> true
                        | _ -> false)
                      name
                | _ -> false
              in
              let value =
                if is_positional then
                  match int_of_string_opt name with
                  | None -> Error ("invalid positional parameter: " ^ name)
                  | Some 0 -> Ok None
                  | Some position ->
                      Ok (List.nth_opt context.script_arguments (position - 1))
                else if valid_named then
                  Ok (List.assoc_opt name context.variables)
                else
                  Error
                    ("unsupported parameter expression: ${" ^ expression ^ "}")
              in
              begin match value with
              | Error _ as error -> error
              | Ok value ->
                  let value =
                    match (value, default) with
                    | (None | Some ""), Some fallback -> Some fallback
                    | value, _ -> value
                  in
                  begin match value with
                  | None -> Error ("unbound variable: " ^ name)
                  | Some value ->
                      Buffer.add_string actual value;
                      Buffer.add_string redacted
                        (if List.mem name context.secret_names then
                           "<secret:" ^ name ^ ">"
                         else value);
                      loop (close + 1)
                  end
              end
          end
    else begin
      Buffer.add_char actual text.[index];
      Buffer.add_char redacted text.[index];
      loop (index + 1)
    end
  in
  loop 0

let expand_list context values =
  let rec loop actual redacted = function
    | [] -> Ok (List.rev actual, List.rev redacted)
    | value :: rest ->
        begin match expand_text context value with
        | Error _ as error -> error
        | Ok (actual_value, redacted_value) ->
            loop (actual_value :: actual) (redacted_value :: redacted) rest
        end
  in
  loop [] [] values

let expand_pairs context pairs =
  let rec loop actual redacted = function
    | [] -> Ok (List.rev actual, List.rev redacted)
    | (name, value) :: rest ->
        begin match expand_text context value with
        | Error _ as error -> error
        | Ok (actual_value, redacted_value) ->
            loop
              ((name, actual_value) :: actual)
              ((name, redacted_value) :: redacted)
              rest
        end
  in
  loop [] [] pairs

let lowercase = String.lowercase_ascii

type powershell_argument_origin =
  | Typed_input
  | Default_value
  | Positional_argument
  | Separate_named_argument
  | Inline_named_argument
  | Switch_presence

let rec normalize_invocation_value name value_type origin value =
  let type_error expected =
    Error (Printf.sprintf "PowerShell parameter -%s expects %s" name expected)
  in
  match value_type with
  | Ir.Text | Ir.Path | Ir.Bytes -> Ok value
  | Ir.Int ->
      begin match Ir.normalize_powershell_int32 value with
      | Ok value -> Ok value
      | Error _ -> type_error "a PowerShell Int32"
      end
  | Ir.Bool ->
      begin match (origin, lowercase value) with
      | ( (Inline_named_argument | Switch_presence | Default_value),
          ("true" | "$true") ) ->
          Ok "True"
      | ( (Inline_named_argument | Switch_presence | Default_value),
          ("false" | "$false") ) ->
          Ok "False"
      | Default_value, "1" -> Ok "True"
      | Default_value, "0" -> Ok "False"
      | Typed_input, ("true" | "$true" | "1") -> Ok "True"
      | Typed_input, ("false" | "$false" | "0") -> Ok "False"
      | (Positional_argument | Separate_named_argument), _ ->
          Error
            (Printf.sprintf
               "PowerShell parameter -%s boolean values from process arguments \
                require -%s:true or -%s:false colon syntax"
               name name name)
      | (Inline_named_argument | Switch_presence | Default_value), _ ->
          type_error "a boolean literal true or false"
      | Typed_input, _ -> type_error "a boolean"
      end
  | Ir.Secret inner -> normalize_invocation_value name inner origin value
  | Ir.List _ | Ir.Record _ | Ir.Byte_stream | Ir.Object_stream _ ->
      type_error "a scalar value supported by the internal runner"

let rec invocation_base_type = function
  | Ir.Secret inner -> invocation_base_type inner
  | value_type -> value_type

let validate_invocation_value parameter value_type value =
  let fail detail =
    Error
      (Printf.sprintf "PowerShell parameter -%s %s" parameter.Ir.input detail)
  in
  let allows_empty = List.mem Ir.Allow_empty_string parameter.Ir.validations in
  let* () =
    if
      parameter.required && value = ""
      && invocation_base_type value_type = Ir.Text
      && not allows_empty
    then fail "cannot be an empty string"
    else Ok ()
  in
  let rec validate = function
    | [] -> Ok ()
    | Ir.Allow_empty_string :: rest -> validate rest
    | Ir.Not_null_or_empty :: _ when value = "" ->
        fail "failed ValidateNotNullOrEmpty"
    | Ir.Not_null_or_empty :: rest -> validate rest
    | Ir.String_set { values; ignore_case } :: rest ->
        let equal left right =
          if ignore_case then String.equal (lowercase left) (lowercase right)
          else String.equal left right
        in
        if List.exists (equal value) values then validate rest
        else
          fail
            (Printf.sprintf "failed ValidateSet(%s)"
               (String.concat ", " values))
    | Ir.Int_range { minimum; maximum } :: rest ->
        begin match int_of_string_opt value with
        | Some number when number >= minimum && number <= maximum ->
            validate rest
        | Some _ | None ->
            fail (Printf.sprintf "is outside range %d..%d" minimum maximum)
        end
  in
  validate parameter.validations

let rec default_invocation_value value_type =
  match value_type with
  | Ir.Int -> Some "0"
  | Ir.Bool -> Some "False"
  | Ir.Text | Ir.Path | Ir.Bytes -> Some ""
  | Ir.Secret inner -> default_invocation_value inner
  | Ir.List _ | Ir.Record _ | Ir.Byte_stream | Ir.Object_stream _ -> None

type powershell_common_parameter_kind =
  | Common_switch
  | Common_action_preference
  | Common_nonnegative_integer
  | Common_variable

type powershell_common_parameter = {
  common_name : string;
  aliases : string list;
  common_kind : powershell_common_parameter_kind;
}

type powershell_named_parameter =
  | Task_parameter of Ir.invocation_parameter
  | Common_parameter of powershell_common_parameter

let powershell_common_parameters =
  [
    { common_name = "Verbose"; aliases = [ "vb" ]; common_kind = Common_switch };
    { common_name = "Debug"; aliases = [ "db" ]; common_kind = Common_switch };
    {
      common_name = "ErrorAction";
      aliases = [ "ea" ];
      common_kind = Common_action_preference;
    };
    {
      common_name = "WarningAction";
      aliases = [ "wa" ];
      common_kind = Common_action_preference;
    };
    {
      common_name = "InformationAction";
      aliases = [ "infa" ];
      common_kind = Common_action_preference;
    };
    {
      common_name = "ProgressAction";
      aliases = [ "proga" ];
      common_kind = Common_action_preference;
    };
    {
      common_name = "ErrorVariable";
      aliases = [ "ev" ];
      common_kind = Common_variable;
    };
    {
      common_name = "WarningVariable";
      aliases = [ "wv" ];
      common_kind = Common_variable;
    };
    {
      common_name = "InformationVariable";
      aliases = [ "iv" ];
      common_kind = Common_variable;
    };
    {
      common_name = "OutVariable";
      aliases = [ "ov" ];
      common_kind = Common_variable;
    };
    {
      common_name = "OutBuffer";
      aliases = [ "ob" ];
      common_kind = Common_nonnegative_integer;
    };
    {
      common_name = "PipelineVariable";
      aliases = [ "pv" ];
      common_kind = Common_variable;
    };
  ]

let validate_powershell_common_parameter parameter value =
  let fail detail =
    Error
      (Printf.sprintf "PowerShell common parameter -%s %s" parameter.common_name
         detail)
  in
  match parameter.common_kind with
  | Common_switch ->
      begin match lowercase value with
      | "true" | "$true" | "false" | "$false" -> Ok ()
      | _ -> fail "expects a boolean switch value"
      end
  | Common_action_preference ->
      let accepted =
        [
          "silentlycontinue";
          "stop";
          "continue";
          "inquire";
          "ignore";
          "suspend";
          "break";
        ]
      in
      if List.mem (lowercase value) accepted then Ok ()
      else fail "expects a valid ActionPreference value"
  | Common_nonnegative_integer ->
      begin match int_of_string_opt value with
      | Some number when number >= 0 -> Ok ()
      | Some _ | None -> fail "expects a non-negative integer"
      end
  | Common_variable ->
      if value = "" then fail "expects a variable name" else Ok ()

let bind_powershell_invocation (task : Ir.task) invocation ~provided arguments =
  let parameters = invocation.Ir.parameters in
  let binding parameter =
    List.find_opt
      (fun (binding : Ir.binding) -> binding.name = parameter.Ir.input)
      task.inputs
  in
  let find_parameter name =
    let normalized = lowercase name in
    let common_parameters =
      if invocation.Ir.accepts_common_parameters then
        powershell_common_parameters
      else []
    in
    let task_exact =
      parameters
      |> List.filter (fun parameter ->
          lowercase parameter.Ir.input = normalized)
      |> List.map (fun parameter -> Task_parameter parameter)
    in
    let common_exact =
      common_parameters
      |> List.filter (fun parameter ->
          lowercase parameter.common_name = normalized
          || List.mem normalized (List.map lowercase parameter.aliases))
      |> List.map (fun parameter -> Common_parameter parameter)
    in
    let exact = task_exact @ common_exact in
    match exact with
    | [ parameter ] -> Ok parameter
    | _ :: _ :: _ -> Error ("ambiguous PowerShell parameter: -" ^ name)
    | [] ->
        let task_candidates =
          parameters
          |> List.filter (fun parameter ->
              String.starts_with ~prefix:normalized
                (lowercase parameter.Ir.input))
          |> List.map (fun parameter -> Task_parameter parameter)
        in
        let common_candidates =
          common_parameters
          |> List.filter (fun parameter ->
              String.starts_with ~prefix:normalized
                (lowercase parameter.common_name))
          |> List.map (fun parameter -> Common_parameter parameter)
        in
        begin match task_candidates @ common_candidates with
        | [ parameter ] -> Ok parameter
        | [] -> Error ("unknown PowerShell parameter: -" ^ name)
        | _ -> Error ("ambiguous PowerShell parameter: -" ^ name)
        end
  in
  let split_named_argument value =
    if
      String.length value < 2
      || value.[0] <> '-'
      || match value.[1] with '0' .. '9' -> true | _ -> false
    then None
    else
      let body = String.sub value 1 (String.length value - 1) in
      match String.index_opt body ':' with
      | None -> Some (body, None)
      | Some separator ->
          Some
            ( String.sub body 0 separator,
              Some
                (String.sub body (separator + 1)
                   (String.length body - separator - 1)) )
  in
  let bound =
    ref (List.map (fun (name, value) -> (name, (value, Typed_input))) provided)
  in
  let bound_common = ref [] in
  let bind parameter value origin =
    if List.mem_assoc parameter.Ir.input !bound then
      Error
        (Printf.sprintf "PowerShell parameter -%s was specified more than once"
           parameter.input)
    else begin
      bound := (parameter.input, (value, origin)) :: !bound;
      Ok ()
    end
  in
  let bind_common parameter value =
    let normalized = lowercase parameter.common_name in
    if List.mem normalized !bound_common then
      Error
        (Printf.sprintf
           "PowerShell common parameter -%s was specified more than once"
           parameter.common_name)
    else
      let* () = validate_powershell_common_parameter parameter value in
      bound_common := normalized :: !bound_common;
      Ok ()
  in
  let next_positional () =
    parameters
    |> List.filter_map (fun parameter ->
        Option.map (fun position -> (position, parameter)) parameter.Ir.position)
    |> List.sort (fun (left, _) (right, _) -> Int.compare left right)
    |> List.find_opt (fun (_, parameter) ->
        not (List.mem_assoc parameter.Ir.input !bound))
    |> Option.map snd
  in
  let rec parse = function
    | [] -> Ok ()
    | argument :: rest ->
        begin match split_named_argument argument with
        | None ->
            begin match next_positional () with
            | None ->
                Error ("unexpected positional PowerShell argument: " ^ argument)
            | Some parameter ->
                let* () = bind parameter argument Positional_argument in
                parse rest
            end
        | Some (name, inline_value) ->
            let* named_parameter = find_parameter name in
            begin match named_parameter with
            | Task_parameter parameter when parameter.is_switch ->
                let value = Option.value ~default:"true" inline_value in
                let origin =
                  if Option.is_some inline_value then Inline_named_argument
                  else Switch_presence
                in
                let* () = bind parameter value origin in
                parse rest
            | Task_parameter parameter ->
                begin match (inline_value, rest) with
                | Some value, _ ->
                    let* () = bind parameter value Inline_named_argument in
                    parse rest
                | None, value :: remaining ->
                    let* () = bind parameter value Separate_named_argument in
                    parse remaining
                | None, [] ->
                    Error
                      (Printf.sprintf
                         "PowerShell parameter -%s requires a value"
                         parameter.input)
                end
            | Common_parameter parameter
              when parameter.common_kind = Common_switch ->
                let value = Option.value ~default:"true" inline_value in
                let* () = bind_common parameter value in
                parse rest
            | Common_parameter parameter ->
                begin match (inline_value, rest) with
                | Some value, _ ->
                    let* () = bind_common parameter value in
                    parse rest
                | None, value :: remaining ->
                    let* () = bind_common parameter value in
                    parse remaining
                | None, [] ->
                    Error
                      (Printf.sprintf
                         "PowerShell common parameter -%s requires a value"
                         parameter.common_name)
                end
            end
        end
  in
  let* () = parse arguments in
  let rec finalize accumulator = function
    | [] -> Ok (List.rev accumulator)
    | parameter :: rest ->
        let raw = List.assoc_opt parameter.Ir.input !bound in
        let* raw, origin, validate_input =
          match raw with
          | Some (value, origin) -> Ok (value, origin, true)
          | None when parameter.required ->
              Error
                (Printf.sprintf "missing mandatory PowerShell parameter -%s"
                   parameter.input)
          | None ->
              begin match (parameter.default, binding parameter) with
              | Some value, _ -> Ok (value, Default_value, false)
              | None, Some binding ->
                  begin match default_invocation_value binding.value_type with
                  | Some value -> Ok (value, Default_value, false)
                  | None ->
                      Error
                        (Printf.sprintf
                           "PowerShell parameter -%s has no representable \
                            default"
                           parameter.input)
                  end
              | None, None ->
                  Error
                    (Printf.sprintf
                       "PowerShell parameter -%s refers to an unknown task \
                        input"
                       parameter.input)
              end
        in
        begin match binding parameter with
        | None ->
            Error
              (Printf.sprintf
                 "PowerShell parameter -%s refers to an unknown task input"
                 parameter.input)
        | Some binding ->
            let* value =
              normalize_invocation_value parameter.input binding.value_type
                origin raw
            in
            let* () =
              if validate_input then
                validate_invocation_value parameter binding.value_type value
              else Ok ()
            in
            finalize ((parameter.input, value) :: accumulator) rest
        end
  in
  finalize [] parameters

let bind_task_invocation task ~provided arguments =
  match task.Ir.invocation with
  | None -> Ok provided
  | Some ({ style = Ir.Powershell; _ } as invocation) ->
      bind_powershell_invocation task invocation ~provided arguments

let rec normalize_runtime_state_value name value_type value =
  let invalid expected =
    Error
      (Printf.sprintf "runtime variable %s must be a valid %s" name expected)
  in
  match value_type with
  | Ir.Text | Ir.Path | Ir.Bytes -> Ok value
  | Ir.Int ->
      begin match int_of_string_opt (String.trim value) with
      | Some value -> Ok (string_of_int value)
      | None -> invalid "integer"
      end
  | Ir.Bool ->
      begin match String.lowercase_ascii (String.trim value) with
      | "true" | "1" -> Ok "true"
      | "false" | "0" -> Ok "false"
      | _ -> invalid "boolean"
      end
  | Ir.Secret inner -> normalize_runtime_state_value name inner value
  | Ir.List _ | Ir.Record _ | Ir.Byte_stream | Ir.Object_stream _ ->
      invalid "scalar value"

let rec runtime_state_is_secret = function
  | Ir.Secret _ -> true
  | Ir.Bytes | Ir.Text | Ir.Bool | Ir.Int | Ir.Path | Ir.List _ | Ir.Record _
  | Ir.Byte_stream | Ir.Object_stream _ ->
      false

let trim_trailing_newlines value =
  let finish = ref (String.length value) in
  while !finish > 0 && value.[!finish - 1] = '\n' do
    decr finish
  done;
  if !finish = String.length value then value else String.sub value 0 !finish

let run_plan_with_inputs ~backend ~policy ~inputs ?(arguments = []) plan =
  let script_arguments = arguments in
  let tasks = Hashtbl.create (List.length plan.Ir.tasks) in
  List.iter (fun task -> Hashtbl.replace tasks task.Ir.name task) plan.tasks;
  let rec run_task stack stdin arguments name =
    if List.mem name stack then
      Error
        ("recursive task call detected: "
        ^ String.concat " -> " (List.rev (name :: stack)))
    else
      match Hashtbl.find_opt tasks name with
      | None -> Error ("task not found: " ^ name)
      | Some task ->
          let inherited_environment =
            List.filter
              (fun (variable, _) -> List.mem variable task.Ir.environment)
              inputs
          in
          let variables =
            List.fold_left add_variable inherited_environment arguments
          in
          let missing =
            List.filter_map
              (fun (binding : Ir.binding) ->
                if List.mem_assoc binding.name variables then None
                else Some binding.name)
              task.inputs
          in
          if missing <> [] then
            Error
              (Printf.sprintf "task %s is missing input %s" name
                 (String.concat ", " missing))
          else
            let accepted =
              List.map (fun (binding : Ir.binding) -> binding.name) task.inputs
              @ task.environment
            in
            let unexpected =
              List.filter_map
                (fun (argument, _) ->
                  if List.mem argument accepted then None else Some argument)
                arguments
            in
            if unexpected <> [] then
              Error
                (Printf.sprintf "task %s received unknown input %s" name
                   (String.concat ", " unexpected))
            else
              run_node (name :: stack)
                { variables; secret_names = task.secrets; script_arguments }
                stdin task.body
              |> Result.map fst
  and run_nodes_sequence stack context stdin nodes =
    let rec loop context input accumulator = function
      | [] -> Ok (accumulator, context)
      | node :: rest ->
          begin match run_node stack context input node with
          | Error _ as error -> error
          | Ok (current, context) ->
              let combined =
                combine ~exit_code:current.exit_code accumulator current
              in
              loop context "" combined rest
          end
    in
    loop context stdin empty nodes
  and run_nodes_pipeline stack context stdin nodes =
    let rec loop input stderr trace last = function
      | [] ->
          Ok
            ( { exit_code = last.exit_code; stdout = last.stdout; stderr; trace },
              context )
      | node :: rest ->
          begin match run_node stack context input node with
          | Error _ as error -> error
          | Ok (current, _) ->
              loop current.stdout (stderr ^ current.stderr)
                (trace @ current.trace) current rest
          end
    in
    match nodes with
    | [] -> Ok (empty, context)
    | first :: rest ->
        begin match run_node stack context stdin first with
        | Error _ as error -> error
        | Ok (current, _) ->
            loop current.stdout current.stderr current.trace current rest
        end
  and run_parallel stack context stdin nodes =
    let domains =
      List.map
        (fun node -> Domain.spawn (fun () -> run_node stack context stdin node))
        nodes
    in
    let results = List.map Domain.join domains in
    let rec merge accumulator = function
      | [] -> Ok (accumulator, context)
      | Error message :: _ -> Error message
      | Ok (current, _) :: rest ->
          merge (combine ~exit_code:current.exit_code accumulator current) rest
    in
    merge empty results
  and run_node stack context stdin (node : Ir.node) =
    match node.operation with
    | Ir.Exec command ->
        begin match expand_list context command.argv with
        | Error _ as error -> error
        | Ok (argv, trace_argv) ->
            begin match command_policy_error policy argv with
            | Some message -> Error message
            | None ->
                begin match expand_pairs context command.environment with
                | Error _ as error -> error
                | Ok (environment, _) ->
                    begin match
                      match command.working_directory with
                      | None -> Ok None
                      | Some value ->
                          Result.map
                            (fun (actual, _) -> Some actual)
                            (expand_text context value)
                    with
                    | Error _ as error -> error
                    | Ok working_directory ->
                        begin match
                          backend.execute
                            { argv; environment; working_directory; stdin }
                          |> redact_backend_error context
                        with
                        | Error _ as error -> error
                        | Ok result ->
                            Ok (process_observation ~trace_argv result, context)
                        end
                    end
                end
            end
        end
    | Ir.Pipeline nodes -> run_nodes_pipeline stack context stdin nodes
    | Ir.Sequence nodes -> run_nodes_sequence stack context stdin nodes
    | Ir.Parallel nodes -> run_parallel stack context stdin nodes
    | Ir.Condition { predicate; if_true; if_false } ->
        begin match run_node stack context stdin predicate with
        | Error _ as error -> error
        | Ok (condition, predicate_context) ->
            let branch =
              if condition.exit_code = 0 then Some if_true else if_false
            in
            begin match branch with
            | None -> Ok (condition, predicate_context)
            | Some branch ->
                begin match run_node stack predicate_context "" branch with
                | Error _ as error -> error
                | Ok (result, branch_context) ->
                    Ok
                      ( combine ~exit_code:result.exit_code condition result,
                        branch_context )
                end
            end
        end
    | Ir.Match { value; cases; default } ->
        begin match expand_text context value with
        | Error _ as error -> error
        | Ok (value, _) ->
            let branch =
              match List.assoc_opt value cases with
              | Some branch -> Some branch
              | None -> default
            in
            Option.fold
              ~none:(Ok (empty, context))
              ~some:(run_node stack context stdin)
              branch
        end
    | Ir.For_each { variable; body; items } ->
        begin match expand_list context items with
        | Error _ as error -> error
        | Ok (items, _) ->
            let previous = List.assoc_opt variable context.variables in
            let rec loop context accumulator = function
              | [] ->
                  Ok
                    ( accumulator,
                      {
                        context with
                        variables =
                          restore_variable context.variables variable previous;
                      } )
              | item :: rest ->
                  let iteration_context =
                    {
                      context with
                      variables = add_variable context.variables (variable, item);
                    }
                  in
                  begin match run_node stack iteration_context stdin body with
                  | Error _ as error -> error
                  | Ok (current, next_context) ->
                      loop next_context
                        (combine ~exit_code:current.exit_code accumulator
                           current)
                        rest
                  end
            in
            loop context empty items
        end
    | Ir.Try_finally { body; finalizer } ->
        begin match run_node stack context stdin body with
        | Error body_error ->
            begin match run_node stack context "" finalizer with
            | Ok _ -> Error body_error
            | Error finalizer_error ->
                Error
                  (body_error ^ "; finalizer also failed: " ^ finalizer_error)
            end
        | Ok (body_result, body_context) ->
            begin match run_node stack body_context "" finalizer with
            | Error _ as error -> error
            | Ok (finalizer_result, finalizer_context) ->
                let exit_code =
                  if finalizer_result.exit_code <> 0 then
                    finalizer_result.exit_code
                  else body_result.exit_code
                in
                Ok
                  ( combine ~exit_code body_result finalizer_result,
                    finalizer_context )
            end
        end
    | Ir.Task_call { task; arguments } ->
        begin match expand_pairs context arguments with
        | Error _ as error -> error
        | Ok (arguments, _) ->
            Result.map
              (fun observation -> (observation, context))
              (run_task stack stdin arguments task)
        end
    | Ir.Set_variable assignment ->
        begin match expand_text context assignment.value with
        | Error _ as error -> error
        | Ok (value, _) ->
            begin match
              normalize_runtime_state_value assignment.name
                assignment.value_type value
            with
            | Error _ as error -> error
            | Ok value ->
                let secret_names =
                  if runtime_state_is_secret assignment.value_type then
                    assignment.name
                    :: List.filter
                         (fun name -> name <> assignment.name)
                         context.secret_names
                  else context.secret_names
                in
                Ok
                  ( empty,
                    {
                      context with
                      variables =
                        add_variable context.variables (assignment.name, value);
                      secret_names;
                    } )
            end
        end
    | Ir.Capture_stdout capture ->
        begin match run_node stack context stdin capture.body with
        | Error _ as error -> error
        | Ok (captured, _) ->
            let value = trim_trailing_newlines captured.stdout in
            begin match
              normalize_runtime_state_value capture.name capture.value_type
                value
            with
            | Error _ as error -> error
            | Ok value ->
                Ok
                  ( { captured with stdout = "" },
                    {
                      context with
                      variables =
                        add_variable context.variables (capture.name, value);
                    } )
            end
        end
    | Ir.File_read path ->
        if not policy.allow_file_read then
          Error ("file read denied by policy: " ^ path)
        else
          begin match expand_text context path with
          | Error _ as error -> error
          | Ok (path, trace_path) ->
              begin match
                backend.read_file path |> redact_backend_error context
              with
              | Error _ as error -> error
              | Ok contents ->
                  Ok
                    ( {
                        exit_code = 0;
                        stdout = contents;
                        stderr = "";
                        trace = [ File_read trace_path ];
                      },
                      context )
              end
          end
    | Ir.File_write write ->
        if not policy.allow_file_write then
          Error ("file write denied by policy: " ^ write.path)
        else
          begin match expand_text context write.path with
          | Error _ as error -> error
          | Ok (path, trace_path) ->
              begin match expand_text context write.contents with
              | Error _ as error -> error
              | Ok (contents, _) ->
                  begin match
                    backend.write_file ~path ~contents ~append:write.append
                    |> redact_backend_error context
                  with
                  | Error _ as error -> error
                  | Ok () ->
                      Ok
                        ( { empty with trace = [ File_write trace_path ] },
                          context )
                  end
              end
          end
    | Ir.File_remove path ->
        if not policy.allow_file_write then
          Error ("file remove denied by policy: " ^ path)
        else
          begin match expand_text context path with
          | Error _ as error -> error
          | Ok (path, trace_path) ->
              begin match
                backend.remove_file path |> redact_backend_error context
              with
              | Error _ as error -> error
              | Ok () ->
                  Ok ({ empty with trace = [ File_remove trace_path ] }, context)
              end
          end
    | Ir.Network_request request ->
        if not policy.allow_network then
          Error ("network denied by policy: " ^ request.uri)
        else
          begin match expand_text context request.method_ with
          | Error _ as error -> error
          | Ok (method_, trace_method) ->
              begin match expand_text context request.uri with
              | Error _ as error -> error
              | Ok (uri, trace_uri) ->
                  begin match
                    backend.network_request ~method_ ~uri
                    |> redact_backend_error context
                  with
                  | Error _ as error -> error
                  | Ok body ->
                      Ok
                        ( {
                            exit_code = 0;
                            stdout = body;
                            stderr = "";
                            trace = [ Network (trace_method, trace_uri) ];
                          },
                          context )
                  end
              end
          end
    | Ir.Opaque_capsule capsule ->
        if not policy.allow_opaque then
          Error ("opaque capsule execution denied by policy: " ^ node.id)
        else
          let argv = capsule_argv capsule context.script_arguments in
          begin match
            backend.execute
              { argv; environment = []; working_directory = None; stdin }
            |> redact_backend_error context
          with
          | Error _ as error -> error
          | Ok result ->
              let observation = process_observation ~trace_argv:argv result in
              Ok
                ( {
                    observation with
                    trace = Capsule node.id :: observation.trace;
                  },
                  context )
          end
  in
  match Ir.validate_plan plan with
  | Error errors -> Error (String.concat "; " errors)
  | Ok () ->
      let entry_task = Hashtbl.find_opt tasks plan.entrypoint in
      let entry_input_names =
        match entry_task with
        | None -> []
        | Some task ->
            List.map (fun (binding : Ir.binding) -> binding.name) task.inputs
      in
      let case_insensitive_entry_inputs =
        match entry_task with
        | Some { Ir.invocation = Some { style = Ir.Powershell; _ }; _ } -> true
        | Some _ | None -> false
      in
      let canonical_input_name name =
        if not case_insensitive_entry_inputs then name
        else
          entry_input_names
          |> List.find_opt (fun candidate ->
              String.equal (lowercase candidate) (lowercase name))
          |> Option.value ~default:name
      in
      let inputs =
        List.map
          (fun (name, value) -> (canonical_input_name name, value))
          inputs
      in
      let environment_names =
        List.concat_map (fun task -> task.Ir.environment) plan.tasks
      in
      let accepted = entry_input_names @ environment_names in
      let rec first_duplicate seen = function
        | [] -> None
        | (name, _) :: rest ->
            if List.mem name seen then Some name
            else first_duplicate (name :: seen) rest
      in
      begin match first_duplicate [] inputs with
      | Some name -> Error ("duplicate plan input " ^ name)
      | None ->
          let unknown =
            List.filter_map
              (fun (name, _) ->
                if List.mem name accepted then None else Some name)
              inputs
          in
          if unknown <> [] then
            Error ("unknown plan input " ^ String.concat ", " unknown)
          else
            let provided_entry_arguments =
              List.filter
                (fun (name, _) -> List.mem name entry_input_names)
                inputs
            in
            begin match entry_task with
            | None -> Error ("task not found: " ^ plan.entrypoint)
            | Some entry_task ->
                let* entry_arguments =
                  bind_task_invocation entry_task
                    ~provided:provided_entry_arguments arguments
                in
                run_task [] "" entry_arguments plan.entrypoint
            end
      end

let run_plan ~backend ~policy plan =
  run_plan_with_inputs ~backend ~policy ~inputs:[] plan
