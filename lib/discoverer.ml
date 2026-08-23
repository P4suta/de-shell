type classification = Shell | Embedded | Candidate

type location = {
  id : string;
  path : string;
  locator : string option;
  classification : classification;
  interpreter : string option;
  command : string option;
  source_hash : string;
  container_hash : string;
}

type edge = { caller : string; callee : string }
type inventory = { locations : location list; edges : edge list }
type patch_result = { applied : bool; preview : string; files : string list }

type prepared_patch = {
  proposals : Atomic_patch.proposal list;
  preview : string;
  files : string list;
}

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let classification_of_kind = function
  | Scanner.Shell_file -> Shell
  | Scanner.Embedded_shell -> Embedded
  | Scanner.Candidate -> Candidate

let locator_line locator =
  match String.rindex_opt locator ':' with
  | None -> None
  | Some separator ->
      begin try
        Some
          (int_of_string
             (String.sub locator (separator + 1)
                (String.length locator - separator - 1)))
      with Failure _ -> None
      end

let line_at content line_number =
  if line_number <= 0 then None
  else List.nth_opt (String.split_on_char '\n' content) (line_number - 1)

let strip_outer_quotes value =
  let value = String.trim value in
  let length = String.length value in
  if length >= 2 then
    match (value.[0], value.[length - 1]) with
    | '"', '"' | '\'', '\'' -> String.sub value 1 (length - 2)
    | _ -> value
  else value

let command_for_package content locator =
  let prefix = "scripts." in
  if not (String.starts_with ~prefix locator) then None
  else
    let name =
      String.sub locator (String.length prefix)
        (String.length locator - String.length prefix)
    in
    try
      match Yojson.Safe.from_string content with
      | `Assoc fields ->
          begin match List.assoc_opt "scripts" fields with
          | Some (`Assoc scripts) ->
              begin match List.assoc_opt name scripts with
              | Some (`String value) -> Some value
              | _ -> None
              end
          | _ -> None
          end
      | _ -> None
    with Yojson.Json_error _ -> None

let vscode_task_index locator =
  let prefix = "tasks." in
  let suffix = ".command" in
  if
    (not (String.starts_with ~prefix locator))
    || not (String.ends_with ~suffix locator)
  then None
  else
    let start = String.length prefix in
    let length = String.length locator - start - String.length suffix in
    if length <= 0 then None
    else
      try Some (int_of_string (String.sub locator start length))
      with Failure _ -> None

let command_for_vscode content locator =
  match vscode_task_index locator with
  | None -> None
  | Some index ->
      begin try
        let document = Scanner.parse_json_relaxed content in
        let tasks =
          Yojson.Safe.Util.member "tasks" document |> Yojson.Safe.Util.to_list
        in
        match List.nth_opt tasks index with
        | Some (`Assoc fields) ->
            begin match List.assoc_opt "command" fields with
            | Some (`String command) ->
                let arguments =
                  match List.assoc_opt "args" fields with
                  | Some (`List values) ->
                      List.filter_map
                        (function `String value -> Some value | _ -> None)
                        values
                  | _ -> []
                in
                Some (String.concat " " (command :: arguments))
            | _ -> None
            end
        | _ -> None
      with Yojson.Json_error _ | Yojson.Safe.Util.Type_error _ -> None
      end

let command_for_line content locator =
  match locator_line locator with
  | None -> None
  | Some line_number ->
      begin match line_at content line_number with
      | None -> None
      | Some line -> (
          if String.starts_with ~prefix:"recipe:" locator then
            Some
              (if String.length line > 0 && line.[0] = '\t' then
                 String.sub line 1 (String.length line - 1)
               else String.trim line)
          else if String.starts_with ~prefix:"RUN:" locator then
            let trimmed = String.trim line in
            if String.length trimmed > 4 then
              Some (String.sub trimmed 4 (String.length trimmed - 4))
            else None
          else
            match String.index_opt line ':' with
            | Some separator ->
                let value =
                  String.sub line (separator + 1)
                    (String.length line - separator - 1)
                  |> strip_outer_quotes
                in
                if
                  value = ""
                  || List.mem value [ "|"; "|-"; "|+"; ">"; ">-"; ">+" ]
                then None
                else Some value
            | None ->
                let trimmed = String.trim line in
                if String.starts_with ~prefix:"-" trimmed then
                  Some
                    (String.sub trimmed 1 (String.length trimmed - 1)
                    |> strip_outer_quotes)
                else Some trimmed)
      end

let command_for_finding root (finding : Scanner.finding) =
  ignore root;
  Some finding.source

let normalize_path path =
  String.map (fun character -> if character = '\\' then '/' else character) path

let location_of_finding root (finding : Scanner.finding) =
  let container_hash =
    try Sha256.file (Filename.concat root finding.path)
    with Sys_error _ -> String.make 64 '0'
  in
  let locator = Option.value ~default:"<file>" finding.locator in
  let id =
    Sha256.hex
      (String.concat "\000"
         [ finding.path; locator; finding.content_hash; container_hash ])
  in
  {
    id;
    path = finding.path;
    locator = finding.locator;
    classification = classification_of_kind finding.kind;
    interpreter = finding.interpreter;
    command = command_for_finding root finding;
    source_hash = finding.content_hash;
    container_hash;
  }

let command_words command =
  let normalized =
    String.map
      (function
        | '"' | '\'' | '\t' | '\r' | '\n' | ';' | '(' | ')' | '|' | '&' -> ' '
        | '\\' -> '/'
        | character -> character)
      command
  in
  normalized |> String.split_on_char ' ' |> List.filter (fun word -> word <> "")

let calls_path command target =
  let target = normalize_path target in
  let basename = Filename.basename target in
  let words = command_words command in
  List.exists
    (fun word ->
      let word =
        if String.starts_with ~prefix:"./" word then
          String.sub word 2 (String.length word - 2)
        else word
      in
      word = target || (word = basename && not (String.contains target '/')))
    words

let command_is_exact_call command target =
  let normalize value =
    let value = normalize_path value in
    if String.starts_with ~prefix:"./" value then
      String.sub value 2 (String.length value - 2)
    else value
  in
  let target = normalize target in
  let words = List.map normalize (command_words command) in
  let launcher = function
    | "sh" | "bash" | "dash" | "ksh" | "zsh" | "fish" | "pwsh" | "powershell"
    | "powershell.exe" | "nu" | "cmd" | "cmd.exe" | "call" | "exec" ->
        true
    | _ -> false
  in
  match words with
  | [ value ] -> value = target
  | [ prefix; value ] ->
      launcher (String.lowercase_ascii prefix) && value = target
  | _ -> false

let discover ~root =
  let locations = Scanner.scan ~root |> List.map (location_of_finding root) in
  let shell_files =
    List.filter (fun location -> location.classification = Shell) locations
  in
  let edges =
    List.concat_map
      (fun caller ->
        match caller.command with
        | None -> []
        | Some command ->
            List.filter_map
              (fun callee ->
                if caller.id <> callee.id && calls_path command callee.path then
                  Some { caller = caller.id; callee = callee.id }
                else None)
              shell_files)
      locations
    |> List.sort_uniq compare
  in
  { locations; edges }

let replace_nth_line lines line_number replacement =
  let rec loop index = function
    | [] -> Error (Printf.sprintf "line %d no longer exists" line_number)
    | _ :: rest when index = line_number -> Ok (replacement :: rest)
    | line :: rest ->
        begin match loop (index + 1) rest with
        | Error _ as error -> error
        | Ok rest -> Ok (line :: rest)
        end
  in
  loop 1 lines

let replace_make_or_docker content locator replacement =
  match locator_line locator with
  | None -> Error ("locator has no line number: " ^ locator)
  | Some line_number ->
      let lines = String.split_on_char '\n' content in
      let original =
        Option.value ~default:"" (List.nth_opt lines (line_number - 1))
      in
      let new_line =
        if String.starts_with ~prefix:"recipe:" locator then
          let prefix =
            if String.length original > 0 && original.[0] = '\t' then "\t"
            else ""
          in
          prefix ^ replacement
        else
          let indentation =
            let rec count index =
              if
                index < String.length original
                && (original.[index] = ' ' || original.[index] = '\t')
              then count (index + 1)
              else index
            in
            count 0
          in
          String.sub original 0 indentation ^ "RUN " ^ replacement
      in
      Result.map (String.concat "\n")
        (replace_nth_line lines line_number new_line)

let replace_yaml_line content locator replacement =
  match locator_line locator with
  | None -> Error ("locator has no line number: " ^ locator)
  | Some line_number ->
      let lines = String.split_on_char '\n' content |> Array.of_list in
      let original =
        if line_number > 0 && line_number <= Array.length lines then
          Some lines.(line_number - 1)
        else None
      in
      begin match original with
      | None -> Error (Printf.sprintf "line %d no longer exists" line_number)
      | Some original ->
          let leading =
            let rec count index =
              if index < String.length original && original.[index] = ' ' then
                count (index + 1)
              else index
            in
            count 0
          in
          let trimmed = String.trim original in
          let block_marker =
            let value =
              match String.index_opt trimmed ':' with
              | Some separator ->
                  String.sub trimmed (separator + 1)
                    (String.length trimmed - separator - 1)
                  |> String.trim
              | None when String.starts_with ~prefix:"-" trimmed ->
                  String.sub trimmed 1 (String.length trimmed - 1)
                  |> String.trim
              | None -> ""
            in
            Scanner.yaml_block_marker value
          in
          let new_line =
            if
              String.starts_with ~prefix:"-" trimmed
              && not (String.contains trimmed ':')
            then String.make leading ' ' ^ "- " ^ replacement
            else
              match String.index_opt original ':' with
              | None -> String.make leading ' ' ^ replacement
              | Some separator ->
                  String.sub original 0 (separator + 1) ^ " " ^ replacement
          in
          let finish =
            if not block_marker then line_number
            else
              let rec loop index =
                if index >= Array.length lines then index
                else
                  let line = lines.(index) in
                  if String.trim line = "" || Scanner.indentation line > leading
                  then loop (index + 1)
                  else index
              in
              loop line_number
          in
          let before = Array.sub lines 0 (line_number - 1) |> Array.to_list in
          let after_start = if block_marker then finish else line_number in
          let after =
            Array.sub lines after_start (Array.length lines - after_start)
            |> Array.to_list
          in
          let output = String.concat "\n" (before @ (new_line :: after)) in
          let output =
            if
              String.ends_with ~suffix:"\n" content
              && not (String.ends_with ~suffix:"\n" output)
            then output ^ "\n"
            else output
          in
          Ok output
      end

let patch_package content replacements =
  try
    match Yojson.Safe.from_string content with
    | `Assoc fields ->
        begin match List.assoc_opt "scripts" fields with
        | Some (`Assoc scripts) ->
            let scripts =
              List.fold_left
                (fun scripts (locator, replacement) ->
                  let name = String.sub locator 8 (String.length locator - 8) in
                  (name, `String replacement) :: List.remove_assoc name scripts)
                scripts replacements
            in
            Ok
              (Yojson.Safe.pretty_to_string
                 (`Assoc
                    (("scripts", `Assoc scripts)
                    :: List.remove_assoc "scripts" fields))
              ^ "\n")
        | _ -> Error "package.json has no scripts object"
        end
    | _ -> Error "package.json must contain an object"
  with Yojson.Json_error message -> Error ("invalid package.json: " ^ message)

let patch_vscode content replacements =
  let rec replace_nth index replacement current = function
    | [] -> Error (Printf.sprintf "VS Code task %d no longer exists" index)
    | _ :: rest when current = index -> Ok (replacement :: rest)
    | value :: rest ->
        Result.map
          (fun rest -> value :: rest)
          (replace_nth index replacement (current + 1) rest)
  in
  try
    match Scanner.parse_json_relaxed content with
    | `Assoc fields ->
        begin match List.assoc_opt "tasks" fields with
        | Some (`List original_tasks) ->
            let rec apply tasks = function
              | [] -> Ok tasks
              | (locator, replacement) :: rest ->
                  begin match vscode_task_index locator with
                  | None -> Error ("invalid VS Code task locator: " ^ locator)
                  | Some index ->
                      begin match List.nth_opt tasks index with
                      | Some (`Assoc task_fields) ->
                          begin match List.assoc_opt "args" task_fields with
                          | Some (`List (_ :: _)) ->
                              Error
                                (Printf.sprintf
                                   "VS Code task %d has arguments that cannot \
                                    be migrated without changing behavior"
                                   index)
                          | Some (`List []) | None ->
                              let task =
                                `Assoc
                                  (("command", `String replacement)
                                  :: List.remove_assoc "command" task_fields)
                              in
                              begin match replace_nth index task 0 tasks with
                              | Error _ as error -> error
                              | Ok tasks -> apply tasks rest
                              end
                          | Some _ ->
                              Error
                                (Printf.sprintf
                                   "VS Code task %d args must be an array" index)
                          end
                      | Some _ ->
                          Error
                            (Printf.sprintf "VS Code task %d must be an object"
                               index)
                      | None ->
                          Error
                            (Printf.sprintf "VS Code task %d no longer exists"
                               index)
                      end
                  end
            in
            Result.map
              (fun tasks ->
                Yojson.Safe.pretty_to_string
                  (`Assoc
                     (("tasks", `List tasks) :: List.remove_assoc "tasks" fields))
                ^ "\n")
              (apply original_tasks replacements)
        | _ -> Error "VS Code tasks.json has no tasks array"
        end
    | _ -> Error "VS Code tasks.json must contain an object"
  with Yojson.Json_error message ->
    Error ("invalid VS Code tasks.json: " ^ message)

let transform_file content locations replacements =
  let first_path = (List.hd locations).path in
  if
    String.ends_with ~suffix:".vscode/tasks.json"
      (normalize_path first_path |> String.lowercase_ascii)
  then
    patch_vscode content
      (List.map
         (fun location ->
           (Option.get location.locator, List.assoc location.id replacements))
         locations)
  else if Filename.basename first_path = "package.json" then
    patch_package content
      (List.map
         (fun location ->
           (Option.get location.locator, List.assoc location.id replacements))
         locations)
  else
    let locations =
      List.sort
        (fun left right ->
          compare
            (Option.bind right.locator locator_line)
            (Option.bind left.locator locator_line))
        locations
    in
    List.fold_left
      (fun result location ->
        match result with
        | Error _ as error -> error
        | Ok content ->
            let replacement = List.assoc location.id replacements in
            begin match (location.classification, location.locator) with
            | Shell, None -> Ok replacement
            | Embedded, Some locator
              when String.starts_with ~prefix:"recipe:" locator
                   || String.starts_with ~prefix:"RUN:" locator ->
                replace_make_or_docker content locator replacement
            | Embedded, Some locator ->
                replace_yaml_line content locator replacement
            | Candidate, _ ->
                Error ("candidate callsite cannot be patched: " ^ location.id)
            | Shell, Some _ ->
                Error ("shell file has an invalid locator: " ^ location.id)
            | _, None -> Error ("location is not patchable: " ^ location.id)
            end)
      (Ok content) locations

let prepare_patch ~root ~inventory ~replacements =
  let replacement_ids = List.map fst replacements in
  if
    List.length replacement_ids
    <> List.length (List.sort_uniq String.compare replacement_ids)
  then Error "duplicate callsite replacement"
  else
    let selected =
      List.filter
        (fun location -> List.mem location.id replacement_ids)
        inventory.locations
    in
    let missing =
      List.filter
        (fun id ->
          not (List.exists (fun location -> location.id = id) selected))
        replacement_ids
    in
    if missing <> [] then
      Error ("unknown callsite: " ^ String.concat ", " missing)
    else
      let paths =
        selected
        |> List.map (fun location -> location.path)
        |> List.sort_uniq String.compare
      in
      let rec build proposals previews = function
        | [] ->
            let proposals = List.rev proposals in
            Ok
              {
                proposals;
                preview = String.concat "" (List.rev previews);
                files = paths;
              }
        | relative :: rest ->
            let absolute = Filename.concat root relative in
            begin try
              let content = read_file absolute in
              let locations =
                List.filter (fun location -> location.path = relative) selected
              in
              let captured_hash = (List.hd locations).container_hash in
              if Sha256.hex content <> captured_hash then
                Error ("content hash mismatch for " ^ relative)
              else
                begin match transform_file content locations replacements with
                | Error _ as error -> error
                | Ok replacement ->
                    let metadata = Unix.lstat absolute in
                    let proposal =
                      Atomic_patch.
                        {
                          path = absolute;
                          expected = Atomic_patch.Existing captured_hash;
                          replacement;
                          permissions = metadata.Unix.st_perm;
                        }
                    in
                    let preview =
                      Project.simple_preview ~path:relative ~before:content
                        ~after:replacement
                    in
                    build (proposal :: proposals) (preview :: previews) rest
                end
            with
            | Sys_error message -> Error message
            | Unix.Unix_error (error, function_name, argument) ->
                Error
                  (Printf.sprintf "%s(%s): %s" function_name argument
                     (Unix.error_message error))
            end
      in
      build [] [] paths

let patch ~root ~inventory ~replacements ~apply =
  match prepare_patch ~root ~inventory ~replacements with
  | Error _ as error -> error
  | Ok prepared ->
      if apply then
        begin match Atomic_patch.apply_all prepared.proposals with
        | Error _ as error -> error
        | Ok () ->
            Ok
              {
                applied = true;
                preview = prepared.preview;
                files = prepared.files;
              }
        end
      else
        Ok
          {
            applied = false;
            preview = prepared.preview;
            files = prepared.files;
          }
