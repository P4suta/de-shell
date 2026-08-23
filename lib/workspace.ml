type entry = { path : string; digest : string; permissions : int }
type snapshot = entry list

type source_entry =
  | Directory of { relative : string; permissions : int }
  | File of {
      relative : string;
      absolute : string;
      permissions : int;
      size : int;
    }

let ignored_directories =
  [ ".git"; ".hg"; ".svn"; ".deshell"; "_build"; "_opam"; "node_modules" ]

let normalize path =
  String.map (fun character -> if character = '\\' then '/' else character) path

let error_of_unix error function_name argument =
  Printf.sprintf "%s(%s): %s" function_name argument (Unix.error_message error)

let enumerate ?(max_bytes = 256 * 1024 * 1024) ?(max_files = 100_000) ~root () =
  try
    let canonical_root = Unix.realpath root in
    let total_bytes = ref 0 in
    let total_files = ref 0 in
    let rec walk relative absolute accumulator =
      let names =
        Sys.readdir absolute |> Array.to_list |> List.sort String.compare
      in
      let rec entries accumulator = function
        | [] -> Ok accumulator
        | name :: rest ->
            let child_absolute = Filename.concat absolute name in
            let child_relative =
              if relative = "" then name else Filename.concat relative name
            in
            let metadata = Unix.lstat child_absolute in
            begin match metadata.Unix.st_kind with
            | Unix.S_LNK ->
                Error
                  ("workspace contains a symlink, which cannot be staged \
                    safely: " ^ normalize child_relative)
            | Unix.S_DIR when List.mem name ignored_directories ->
                entries accumulator rest
            | Unix.S_DIR ->
                begin match
                  walk child_relative child_absolute
                    (Directory
                       {
                         relative = normalize child_relative;
                         permissions = metadata.st_perm;
                       }
                    :: accumulator)
                with
                | Error _ as error -> error
                | Ok accumulator -> entries accumulator rest
                end
            | Unix.S_REG ->
                incr total_files;
                total_bytes := !total_bytes + metadata.st_size;
                if !total_files > max_files then
                  Error
                    (Printf.sprintf
                       "workspace exceeds the %d regular-file staging limit"
                       max_files)
                else if !total_bytes > max_bytes then
                  Error
                    (Printf.sprintf
                       "workspace exceeds the %d byte staging limit" max_bytes)
                else
                  entries
                    (File
                       {
                         relative = normalize child_relative;
                         absolute = child_absolute;
                         permissions = metadata.st_perm;
                         size = metadata.st_size;
                       }
                    :: accumulator)
                    rest
            | Unix.S_CHR | Unix.S_BLK | Unix.S_FIFO | Unix.S_SOCK ->
                Error
                  ("workspace contains an unsupported special file: "
                 ^ normalize child_relative)
            end
      in
      entries accumulator names
    in
    walk "" canonical_root [] |> Result.map List.rev
  with
  | Sys_error message -> Error message
  | Unix.Unix_error (error, function_name, argument) ->
      Error (error_of_unix error function_name argument)

let capture ?max_bytes ?max_files ~root () =
  match enumerate ?max_bytes ?max_files ~root () with
  | Error _ as error -> error
  | Ok entries -> (
      try
        entries
        |> List.filter_map (function
          | Directory _ -> None
          | File file ->
              Some
                {
                  path = file.relative;
                  digest = Sha256.file file.absolute;
                  permissions = file.permissions;
                })
        |> List.sort (fun left right -> String.compare left.path right.path)
        |> fun entries -> Ok entries
      with Sys_error message -> Error message)

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let write_file path contents permissions =
  let channel = open_out_bin path in
  Fun.protect
    ~finally:(fun () -> close_out_noerr channel)
    (fun () -> output_string channel contents);
  Unix.chmod path permissions

let rec ensure_directory path permissions =
  if Sys.file_exists path then ()
  else begin
    let parent = Filename.dirname path in
    if parent <> path then ensure_directory parent 0o700;
    Unix.mkdir path permissions
  end

let safe_fixture_path path =
  let normalized = normalize path in
  Filename.is_relative path && normalized <> ""
  && normalized |> String.split_on_char '/'
     |> List.for_all (fun component ->
         component <> "" && component <> "." && component <> "..")

let stage ?(max_bytes = 256 * 1024 * 1024) ?max_files ~root ~destination
    ~scenario () =
  match enumerate ~max_bytes ?max_files ~root () with
  | Error _ as error -> error
  | Ok entries -> (
      let fixture_bytes =
        match scenario with
        | None -> 0
        | Some (scenario : Scenario.t) ->
            List.fold_left
              (fun total (fixture : Scenario.fixture) ->
                total + String.length fixture.contents)
              0 scenario.fixtures
      in
      let source_bytes =
        List.fold_left
          (fun total -> function
            | Directory _ -> total | File file -> total + file.size)
          0 entries
      in
      if source_bytes + fixture_bytes > max_bytes then
        Error
          (Printf.sprintf "workspace exceeds the %d byte staging limit"
             max_bytes)
      else if Sys.file_exists destination then
        Error ("staging destination already exists: " ^ destination)
      else
        try
          ensure_directory destination 0o700;
          List.iter
            (function
              | Directory directory ->
                  ensure_directory
                    (Filename.concat destination directory.relative)
                    directory.permissions
              | File file ->
                  let target = Filename.concat destination file.relative in
                  ensure_directory (Filename.dirname target) 0o700;
                  write_file target (read_file file.absolute) file.permissions)
            entries;
          begin match scenario with
          | None -> ()
          | Some (scenario : Scenario.t) ->
              List.iter
                (fun (fixture : Scenario.fixture) ->
                  if not (safe_fixture_path fixture.path) then
                    failwith
                      ("fixture path must be project-relative: " ^ fixture.path);
                  let target = Filename.concat destination fixture.path in
                  ensure_directory (Filename.dirname target) 0o700;
                  write_file target fixture.contents
                    (if fixture.executable then 0o700 else 0o600))
                scenario.fixtures
          end;
          Ok ()
        with
        | Failure message | Sys_error message -> Error message
        | Unix.Unix_error (error, function_name, argument) ->
            Error (error_of_unix error function_name argument))

let diff ~before ~after =
  let before_table = Hashtbl.create (List.length before) in
  let after_table = Hashtbl.create (List.length after) in
  List.iter (fun entry -> Hashtbl.replace before_table entry.path entry) before;
  List.iter (fun entry -> Hashtbl.replace after_table entry.path entry) after;
  let paths =
    List.map (fun entry -> entry.path) before
    @ List.map (fun entry -> entry.path) after
    |> List.sort_uniq String.compare
  in
  List.filter_map
    (fun path ->
      let before = Hashtbl.find_opt before_table path in
      let after = Hashtbl.find_opt after_table path in
      match (before, after) with
      | Some left, Some right when left.digest = right.digest -> None
      | _ ->
          Some
            Observation.
              {
                path;
                before = Option.map (fun entry -> entry.digest) before;
                after = Option.map (fun entry -> entry.digest) after;
              })
    paths
