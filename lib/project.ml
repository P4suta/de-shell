type init_result = { created : string list }

type analysis_result = {
  plan : Ir.plan;
  plan_path : string;
  evidence_path : string;
}

type rewrite_result = {
  changed : bool;
  applied : bool;
  preview : string;
  edits : Rewrite.edit list;
}

let ( let* ) result continuation =
  match result with Ok value -> continuation value | Error _ as error -> error

let protect f =
  try Ok (f ()) with
  | Failure message -> Error message
  | Invalid_argument message -> Error message
  | Sys_error message -> Error message
  | Unix.Unix_error (error, function_name, argument) ->
      Error
        (Printf.sprintf "%s(%s): %s" function_name argument
           (Unix.error_message error))
  | Yojson.Json_error message -> Error ("invalid JSON: " ^ message)

let rec ensure_directory path =
  if path = "" || path = "." || Sys.file_exists path then ()
  else begin
    let parent = Filename.dirname path in
    if parent <> path then ensure_directory parent;
    Unix.mkdir path 0o755
  end

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let write_file path contents =
  let directory = Filename.dirname path in
  ensure_directory directory;
  let temporary =
    Filename.temp_file ~temp_dir:directory ".deshell-write-" ".tmp"
  in
  let channel = open_out_bin temporary in
  Fun.protect
    ~finally:(fun () -> close_out_noerr channel)
    (fun () -> output_string channel contents);
  try Unix.rename temporary path
  with error ->
    (try Sys.remove temporary with _ -> ());
    raise error

let write_if_absent path contents =
  if Sys.file_exists path then false
  else begin
    write_file path contents;
    true
  end

let deshell_directory root = Filename.concat root ".deshell"

let project_config root =
  Filename.concat (deshell_directory root) "project.toml"

let load_config ~root =
  match Project_config.load ~root with
  | Ok value -> Ok value
  | Error errors -> Error (String.concat "; " errors)

let configured_entry ~root =
  let* config = load_config ~root in
  match config.Project_config.entrypoints with
  | [ entry ] -> Ok entry
  | [] ->
      Error "no entrypoint was supplied and project.toml entrypoints is empty"
  | _ ->
      Error
        "project.toml contains multiple entrypoints; select one with --entry"

let default_project =
  {|version = 1
entrypoints = []

[policy]
host_write = "deny"
network = "deny"
unknown_interpreter = "trace-only"

[sandbox]
mode = "disposable"

[export]
strict = true
bridge = false
|}

let default_scenario =
  {|name = "default"
args = []

[environment]

[expect]
exit_code = 0
|}

let init ~root =
  protect @@ fun () ->
  ensure_directory root;
  let scenarios = Filename.concat (deshell_directory root) "scenarios" in
  ensure_directory scenarios;
  let files =
    [
      (project_config root, default_project);
      (Filename.concat scenarios "default.toml", default_scenario);
      (Filename.concat root "deshell.lock", Lockfile.default ());
    ]
  in
  let created =
    List.filter_map
      (fun (path, contents) ->
        if write_if_absent path contents then Some path else None)
      files
  in
  { created }

let normalize_path path =
  let path =
    String.map
      (fun character -> if character = '\\' then '/' else character)
      path
  in
  if Sys.win32 then String.lowercase_ascii path else path

let is_within ~root path =
  let root = normalize_path root in
  let path = normalize_path path in
  path = root
  ||
  let prefix = if String.ends_with ~suffix:"/" root then root else root ^ "/" in
  String.starts_with ~prefix path

let resolve_entry ~root entry =
  protect @@ fun () ->
  let canonical_root = Unix.realpath root in
  let candidate =
    if Filename.is_relative entry then Filename.concat canonical_root entry
    else entry
  in
  let canonical_entry = Unix.realpath candidate in
  if not (is_within ~root:canonical_root canonical_entry) then
    failwith ("entrypoint escapes the project root: " ^ entry);
  let metadata = Unix.stat canonical_entry in
  if metadata.Unix.st_kind <> Unix.S_REG then
    failwith ("entrypoint is not a regular file: " ^ entry);
  if metadata.Unix.st_size > 4 * 1024 * 1024 then
    failwith ("entrypoint exceeds the 4 MiB analysis limit: " ^ entry);
  (canonical_root, canonical_entry)

let operation_name (node : Ir.node) =
  match node.operation with
  | Ir.Exec _ -> "exec"
  | Ir.Pipeline _ -> "pipeline"
  | Ir.Sequence _ -> "sequence"
  | Ir.Parallel _ -> "parallel"
  | Ir.Condition _ -> "condition"
  | Ir.Match _ -> "match"
  | Ir.For_each _ -> "foreach"
  | Ir.Try_finally _ -> "try_finally"
  | Ir.Task_call _ -> "task_call"
  | Ir.Set_variable _ -> "set_variable"
  | Ir.Capture_stdout _ -> "capture_stdout"
  | Ir.File_read _ -> "file_read"
  | Ir.File_write _ -> "file_write"
  | Ir.File_remove _ -> "file_remove"
  | Ir.Network_request _ -> "network_request"
  | Ir.Opaque_capsule _ -> "opaque_capsule"

let evidence_json ~entry ~content_hash plan =
  let nodes =
    List.fold_left
      (fun accumulator task ->
        Ir.fold_nodes
          (fun values node ->
            `Assoc
              [
                ("id", `String node.Ir.id);
                ("operation", `String (operation_name node));
                ("guarantee", Ir_codec.encode_guarantee node.guarantee);
              ]
            :: values)
          accumulator task.Ir.body)
      [] plan.Ir.tasks
    |> List.rev
  in
  let encoded_plan = Ir_codec.encode_string plan in
  `Assoc
    [
      ("schema_version", `Int 1);
      ("plan_digest", `String (Sha256.hex encoded_plan));
      ( "source",
        `Assoc
          [ ("path", `String entry); ("content_hash", `String content_hash) ] );
      ("nodes", `List nodes);
    ]

let analyze ~root ~entry =
  let* config = load_config ~root in
  let* _, entry_path = resolve_entry ~root entry in
  protect @@ fun () ->
  let source = read_file entry_path in
  let interpreter = Frontend_registry.detect ~path:entry ~source in
  if
    interpreter = "unknown"
    && config.Project_config.policy.unknown_interpreter = Project_config.Reject
  then
    failwith
      ("unknown interpreter rejected by project policy for entrypoint: " ^ entry);
  let lowered = Frontend_registry.lower ~path:entry source in
  let is_task_input name =
    List.exists
      (fun (input : Ir.binding) ->
        String.equal
          (String.lowercase_ascii input.name)
          (String.lowercase_ascii name))
      lowered.inputs
  in
  let environment =
    Template.environment_variables lowered.root
    |> List.filter (fun name -> not (is_task_input name))
  in
  let secrets = List.filter Concolic.secret_name environment in
  let task =
    Ir.task ~name:"main" ~inputs:lowered.inputs ?invocation:lowered.invocation
      ~environment ~secrets ~body:lowered.root ()
  in
  let plan =
    Ir.plan ~entrypoint:"main" [ task ] |> Command_model.annotate_plan
  in
  begin match Ir.validate_plan plan with
  | Ok () -> ()
  | Error errors -> failwith (String.concat "; " errors)
  end;
  let directory = deshell_directory root in
  ensure_directory directory;
  let plan_path = Filename.concat directory "plan.json" in
  let evidence_path = Filename.concat directory "evidence.json" in
  let plan_contents = Ir_codec.encode_string plan in
  let evidence = evidence_json ~entry ~content_hash:(Sha256.hex source) plan in
  let evidence_contents = Yojson.Safe.pretty_to_string evidence ^ "\n" in
  let prepare path replacement =
    if Sys.file_exists path then Atomic_patch.prepare ~path ~replacement
    else Atomic_patch.prepare_create ~path ~replacement ~permissions:0o644
  in
  begin match
    Atomic_patch.apply_all
      [
        prepare plan_path plan_contents; prepare evidence_path evidence_contents;
      ]
  with
  | Ok () -> ()
  | Error message -> failwith message
  end;
  { plan; plan_path; evidence_path }

let scan ~root =
  protect @@ fun () ->
  let canonical_root = Unix.realpath root in
  Scanner.scan ~root:canonical_root

let check ~root =
  protect @@ fun () ->
  let required_files =
    [
      project_config root;
      Filename.concat root "deshell.lock";
      Filename.concat (deshell_directory root) "plan.json";
      Filename.concat (deshell_directory root) "evidence.json";
    ]
  in
  List.iter
    (fun path ->
      if not (Sys.file_exists path) then
        failwith ("missing required file: " ^ path))
    required_files;
  begin match load_config ~root with
  | Ok _ -> ()
  | Error message -> failwith message
  end;
  begin match Lockfile.load ~root with
  | Ok _ -> ()
  | Error errors -> failwith (String.concat "; " errors)
  end;
  let plan_path = Filename.concat (deshell_directory root) "plan.json" in
  let plan =
    match Ir_codec.decode_string (read_file plan_path) with
    | Ok value -> value
    | Error errors -> failwith (String.concat "; " errors)
  in
  begin match Ir.validate_plan plan with
  | Ok () -> ()
  | Error errors -> failwith (String.concat "; " errors)
  end;
  let evidence_path =
    Filename.concat (deshell_directory root) "evidence.json"
  in
  let evidence = Yojson.Safe.from_file evidence_path in
  let evidence_fields =
    match evidence with
    | `Assoc fields -> fields
    | _ -> failwith "evidence must be a JSON object"
  in
  let required name =
    match List.assoc_opt name evidence_fields with
    | Some value -> value
    | None -> failwith ("evidence is missing required field: " ^ name)
  in
  begin match required "schema_version" with
  | `Int 1 -> ()
  | _ -> failwith "evidence schema_version must be 1"
  end;
  let valid_digest value =
    String.length value = 64
    && String.for_all
         (function '0' .. '9' | 'a' .. 'f' -> true | _ -> false)
         value
  in
  begin match List.assoc_opt "observation" evidence_fields with
  | None -> ()
  | Some (`Assoc fields) ->
      let field name =
        match List.assoc_opt name fields with
        | Some value -> value
        | None ->
            failwith ("evidence observation is missing required field: " ^ name)
      in
      begin match field "requested" with
      | `Bool true -> ()
      | _ -> failwith "evidence observation.requested must be true"
      end;
      let status =
        match field "status" with
        | `String
            (("verified" | "different" | "unavailable" | "failed") as value) ->
            value
        | _ -> failwith "evidence observation.status is invalid"
      in
      let nullable_string name =
        match field name with
        | `String _ | `Null -> ()
        | _ ->
            failwith
              ("evidence observation." ^ name ^ " must be a string or null")
      in
      nullable_string "provider";
      nullable_string "reason";
      let digest =
        match List.assoc_opt "digest" fields with
        | None -> None
        | Some (`String value) when valid_digest value -> Some value
        | Some _ ->
            failwith
              "evidence observation.digest must be a lowercase SHA-256 digest"
      in
      let scenarios =
        match List.assoc_opt "scenarios" fields with
        | None -> None
        | Some (`List (_ :: _ as values))
          when List.for_all
                 (function `String value -> value <> "" | _ -> false)
                 values ->
            Some values
        | Some _ ->
            failwith
              "evidence observation.scenarios must be a non-empty string array"
      in
      if status = "verified" || status = "different" then begin
        if digest = None then
          failwith
            "evidence observation.digest is required for an observed result";
        if scenarios = None then
          failwith
            "evidence observation.scenarios is required for an observed result"
      end
  | Some _ -> failwith "evidence observation must be an object"
  end;
  let recorded_plan_digest =
    match required "plan_digest" with
    | `String value -> value
    | _ -> failwith "evidence plan_digest must be a string"
  in
  let expected_plan_digest = Sha256.hex (Ir_codec.encode_string plan) in
  if recorded_plan_digest <> expected_plan_digest then
    failwith
      (Printf.sprintf "evidence plan digest mismatch (expected %s, found %s)"
         expected_plan_digest recorded_plan_digest);
  let source_path, recorded_source_hash =
    match required "source" with
    | `Assoc fields ->
        let path =
          match List.assoc_opt "path" fields with
          | Some (`String value) -> value
          | _ -> failwith "evidence source.path must be a string"
        in
        let content_hash =
          match List.assoc_opt "content_hash" fields with
          | Some (`String value) -> value
          | _ -> failwith "evidence source.content_hash must be a string"
        in
        (path, content_hash)
    | _ -> failwith "evidence source must be an object"
  in
  let source_file =
    match resolve_entry ~root source_path with
    | Ok (_, path) -> path
    | Error message -> failwith message
  in
  let actual_source_hash = Sha256.file source_file in
  if recorded_source_hash <> actual_source_hash then
    failwith
      (Printf.sprintf "evidence source digest mismatch for %s" source_path);
  let expected_nodes = Hashtbl.create 32 in
  List.iter
    (fun task ->
      Ir.fold_nodes
        (fun () node ->
          Hashtbl.add expected_nodes node.Ir.id
            (operation_name node, node.guarantee))
        () task.Ir.body)
    plan.tasks;
  let evidence_nodes =
    match required "nodes" with
    | `List values -> values
    | _ -> failwith "evidence nodes must be an array"
  in
  let seen = Hashtbl.create (List.length evidence_nodes) in
  List.iter
    (function
      | `Assoc fields ->
          let id =
            match List.assoc_opt "id" fields with
            | Some (`String value) -> value
            | _ -> failwith "evidence node id must be a string"
          in
          if Hashtbl.mem seen id then
            failwith ("duplicate evidence node id: " ^ id);
          Hashtbl.add seen id ();
          let expected_operation, expected_guarantee =
            match Hashtbl.find_opt expected_nodes id with
            | Some value -> value
            | None -> failwith ("unexpected evidence node: " ^ id)
          in
          begin match List.assoc_opt "operation" fields with
          | Some (`String value) when value = expected_operation -> ()
          | _ -> failwith ("evidence operation mismatch for node " ^ id)
          end;
          begin match List.assoc_opt "guarantee" fields with
          | Some value ->
              begin match
                Ir_codec.decode_guarantee
                  ("evidence.nodes." ^ id ^ ".guarantee")
                  value
              with
              | Ok guarantee when guarantee = expected_guarantee -> ()
              | Ok _ -> failwith ("evidence guarantee mismatch for node " ^ id)
              | Error errors -> failwith (String.concat "; " errors)
              end
          | None -> failwith ("missing evidence guarantee for node " ^ id)
          end
      | _ -> failwith "evidence node must be an object")
    evidence_nodes;
  Hashtbl.iter
    (fun id _ ->
      if not (Hashtbl.mem seen id) then failwith ("missing evidence node: " ^ id))
    expected_nodes

let load_plan ~root =
  protect @@ fun () ->
  let path = Filename.concat (deshell_directory root) "plan.json" in
  match Ir_codec.decode_string (read_file path) with
  | Ok plan -> plan
  | Error errors -> failwith (String.concat "; " errors)

let record_observation ~root observation =
  protect @@ fun () ->
  let path = Filename.concat (deshell_directory root) "evidence.json" in
  let evidence = Yojson.Safe.from_string (read_file path) in
  let fields =
    match evidence with
    | `Assoc fields -> fields
    | _ -> failwith "evidence must be a JSON object"
  in
  let updated =
    `Assoc
      (("observation", observation) :: List.remove_assoc "observation" fields)
  in
  write_file path (Yojson.Safe.pretty_to_string updated ^ "\n")

let simple_preview ~path ~before ~after =
  let prefix marker text =
    text |> String.split_on_char '\n'
    |> List.map (fun line -> marker ^ line)
    |> String.concat "\n"
  in
  Printf.sprintf "--- %s\n+++ %s\n@@ -1 +1 @@\n%s\n%s\n" path path
    (prefix "-" before) (prefix "+" after)

let rewrite_equivalent ~root ~entry ~apply =
  let* _, entry_path = resolve_entry ~root entry in
  protect @@ fun () ->
  let before = read_file entry_path in
  let proposal = Atomic_patch.prepare ~path:entry_path ~replacement:before in
  let rewritten = Rewrite.equivalent ~path:entry before in
  let changed = rewritten.output <> before in
  let preview =
    if changed then simple_preview ~path:entry ~before ~after:rewritten.output
    else ""
  in
  if apply && changed then
    begin match
      Atomic_patch.apply { proposal with replacement = rewritten.output }
    with
    | Ok () -> ()
    | Error message -> failwith message
    end;
  { changed; applied = apply && changed; preview; edits = rewritten.edits }
