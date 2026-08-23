let current_schema_version = 1

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
  | File_read of string
  | File_write of file_write
  | File_remove of string
  | Network_request of network_request
  | Opaque_capsule of opaque_capsule

type binding = { name : string; value_type : value_type }

type task = {
  name : string;
  inputs : binding list;
  outputs : binding list;
  environment : string list;
  secrets : string list;
  platform_capabilities : string list;
  cacheable : bool;
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
    ?(platform_capabilities = []) ?(cacheable = false) ~name ~body () =
  {
    name;
    inputs;
    outputs;
    environment;
    secrets;
    platform_capabilities;
    cacheable;
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
      validate_children nodes errors
  | Sequence nodes | Parallel nodes -> validate_children nodes errors
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
          match
            List.find_opt
              (fun (binding : binding) -> binding.name = secret)
              task.inputs
          with
          | None ->
              errors :=
                Printf.sprintf "task %s secret %s is not an input" task.name
                  secret
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
        task.inputs)
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
  | Exec _ | Task_call _ | File_read _ | File_write _ | File_remove _
  | Network_request _ | Opaque_capsule _ ->
      accumulator
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
