type prepared = {
  proposals : Atomic_patch.proposal list;
  preview : string;
  caller_files : string list;
  artifact_path : string option;
}

let normalize_path path =
  String.map (fun character -> if character = '\\' then '/' else character) path

let relative_entry ~root entry =
  let root = Unix.realpath root |> normalize_path in
  let absolute =
    if Filename.is_relative entry then Filename.concat root entry else entry
  in
  let absolute = Unix.realpath absolute |> normalize_path in
  let prefix = if String.ends_with ~suffix:"/" root then root else root ^ "/" in
  if String.starts_with ~prefix absolute then
    String.sub absolute (String.length prefix)
      (String.length absolute - String.length prefix)
  else failwith ("entrypoint escapes project root: " ^ entry)

let prepare_callsites ~root ~entry ~replacement =
  let inventory = Discoverer.discover ~root in
  let entry = relative_entry ~root entry in
  let callee =
    List.find_opt
      (fun (location : Discoverer.location) ->
        location.path = entry
        && location.classification = Discoverer.Shell
        && location.locator = None)
      inventory.locations
  in
  match callee with
  | None -> Error ("entrypoint is missing from repository inventory: " ^ entry)
  | Some callee ->
      let caller_ids =
        inventory.edges
        |> List.filter_map (fun (edge : Discoverer.edge) ->
            if edge.callee = callee.id then Some edge.caller else None)
      in
      let callers =
        inventory.locations
        |> List.filter (fun (location : Discoverer.location) ->
            List.mem location.id caller_ids)
      in
      let unsafe_callers =
        callers
        |> List.filter (fun (location : Discoverer.location) ->
            location.classification <> Discoverer.Embedded
            ||
            match location.command with
            | Some command ->
                not (Discoverer.command_is_exact_call command entry)
            | None -> true)
      in
      if unsafe_callers <> [] then
        let describe (location : Discoverer.location) =
          location.path
          ^ Option.fold ~none:""
              ~some:(fun locator -> "#" ^ locator)
              location.locator
        in
        Error
          ("manual callsite migration is required for: "
          ^ String.concat ", " (List.map describe unsafe_callers))
      else
        let replacements =
          callers
          |> List.filter_map (fun (location : Discoverer.location) ->
              if location.classification = Discoverer.Embedded then
                Some (location.id, replacement)
              else None)
        in
        Discoverer.prepare_patch ~root ~inventory ~replacements

let prepare_artifact ~root (artifact : Exporter.artifact) =
  let path = Filename.concat root artifact.filename in
  if Sys.file_exists path then
    let before = Project.read_file path in
    ( Atomic_patch.prepare ~path ~replacement:artifact.content,
      Project.simple_preview ~path:artifact.filename ~before
        ~after:artifact.content )
  else
    ( Atomic_patch.prepare_create ~path ~replacement:artifact.content
        ~permissions:0o644,
      Project.simple_preview ~path:artifact.filename ~before:""
        ~after:artifact.content )

let prepare ~root ~entry ~artifact =
  try
    match prepare_callsites ~root ~entry ~replacement:"deshell run" with
    | Error _ as error -> error
    | Ok callsites ->
        let artifact_proposals, artifact_preview, artifact_path =
          match artifact with
          | None -> ([], "", None)
          | Some artifact ->
              let proposal, preview = prepare_artifact ~root artifact in
              ([ proposal ], preview, Some proposal.path)
        in
        Ok
          {
            proposals = artifact_proposals @ callsites.proposals;
            preview = artifact_preview ^ callsites.preview;
            caller_files = callsites.files;
            artifact_path;
          }
  with
  | Failure message | Sys_error message -> Error message
  | Unix.Unix_error (error, function_name, argument) ->
      Error
        (Printf.sprintf "%s(%s): %s" function_name argument
           (Unix.error_message error))

let apply prepared = Atomic_patch.apply_all prepared.proposals
