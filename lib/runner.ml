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
    else if index + 1 < length && text.[index] = '$' && text.[index + 1] = '{'
    then
      match find_close (index + 2) with
      | None -> Error ("unterminated variable reference in " ^ text)
      | Some close ->
          let name = String.sub text (index + 2) (close - index - 2) in
          begin match List.assoc_opt name context.variables with
          | None -> Error ("unbound variable: " ^ name)
          | Some value ->
              Buffer.add_string actual value;
              Buffer.add_string redacted
                (if List.mem name context.secret_names then
                   "<secret:" ^ name ^ ">"
                 else value);
              loop (close + 1)
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
  and run_nodes_sequence stack context stdin nodes =
    let rec loop input accumulator = function
      | [] -> Ok accumulator
      | node :: rest ->
          begin match run_node stack context input node with
          | Error _ as error -> error
          | Ok current ->
              let combined =
                combine ~exit_code:current.exit_code accumulator current
              in
              loop "" combined rest
          end
    in
    loop stdin empty nodes
  and run_nodes_pipeline stack context stdin nodes =
    let rec loop input stderr trace last = function
      | [] ->
          Ok { exit_code = last.exit_code; stdout = last.stdout; stderr; trace }
      | node :: rest ->
          begin match run_node stack context input node with
          | Error _ as error -> error
          | Ok current ->
              loop current.stdout (stderr ^ current.stderr)
                (trace @ current.trace) current rest
          end
    in
    match nodes with
    | [] -> Ok empty
    | first :: rest ->
        begin match run_node stack context stdin first with
        | Error _ as error -> error
        | Ok current ->
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
      | [] -> Ok accumulator
      | Error message :: _ -> Error message
      | Ok current :: rest ->
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
                            Ok (process_observation ~trace_argv result)
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
        | Ok condition ->
            let branch =
              if condition.exit_code = 0 then Some if_true else if_false
            in
            begin match branch with
            | None -> Ok condition
            | Some branch ->
                begin match run_node stack context "" branch with
                | Error _ as error -> error
                | Ok result ->
                    Ok (combine ~exit_code:result.exit_code condition result)
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
            Option.fold ~none:(Ok empty)
              ~some:(run_node stack context stdin)
              branch
        end
    | Ir.For_each { variable; body; items } ->
        begin match expand_list context items with
        | Error _ as error -> error
        | Ok (items, _) ->
            let rec loop accumulator = function
              | [] -> Ok accumulator
              | item :: rest ->
                  let context =
                    {
                      context with
                      variables = add_variable context.variables (variable, item);
                    }
                  in
                  begin match run_node stack context stdin body with
                  | Error _ as error -> error
                  | Ok current ->
                      loop
                        (combine ~exit_code:current.exit_code accumulator
                           current)
                        rest
                  end
            in
            loop empty items
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
        | Ok body_result ->
            begin match run_node stack context "" finalizer with
            | Error _ as error -> error
            | Ok finalizer_result ->
                let exit_code =
                  if finalizer_result.exit_code <> 0 then
                    finalizer_result.exit_code
                  else body_result.exit_code
                in
                Ok (combine ~exit_code body_result finalizer_result)
            end
        end
    | Ir.Task_call { task; arguments } ->
        begin match expand_pairs context arguments with
        | Error _ as error -> error
        | Ok (arguments, _) -> run_task stack stdin arguments task
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
                    {
                      exit_code = 0;
                      stdout = contents;
                      stderr = "";
                      trace = [ File_read trace_path ];
                    }
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
                  | Ok () -> Ok { empty with trace = [ File_write trace_path ] }
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
              | Ok () -> Ok { empty with trace = [ File_remove trace_path ] }
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
                        {
                          exit_code = 0;
                          stdout = body;
                          stderr = "";
                          trace = [ Network (trace_method, trace_uri) ];
                        }
                  end
              end
          end
    | Ir.Opaque_capsule capsule ->
        if not policy.allow_opaque then
          Error ("opaque capsule execution denied by policy: " ^ node.id)
        else
          begin match expand_text context capsule.source with
          | Error _ as error -> error
          | Ok (source, trace_source) ->
              let argv =
                capsule_argv { capsule with source } context.script_arguments
              in
              let trace_argv =
                capsule_argv
                  { capsule with source = trace_source }
                  context.script_arguments
              in
              begin match
                backend.execute
                  { argv; environment = []; working_directory = None; stdin }
                |> redact_backend_error context
              with
              | Error _ as error -> error
              | Ok result ->
                  let observation = process_observation ~trace_argv result in
                  Ok
                    {
                      observation with
                      trace = Capsule node.id :: observation.trace;
                    }
              end
          end
  in
  match Ir.validate_plan plan with
  | Error errors -> Error (String.concat "; " errors)
  | Ok () -> run_task [] "" inputs plan.entrypoint

let run_plan ~backend ~policy plan =
  run_plan_with_inputs ~backend ~policy ~inputs:[] plan
