type kind = Shell_file | Embedded_shell | Candidate

type finding = {
  path : string;
  kind : kind;
  interpreter : string option;
  locator : string option;
  content_hash : string;
  source : string;
}

let ignored_directories =
  [
    ".git";
    ".hg";
    ".svn";
    ".deshell";
    "_build";
    "_opam";
    "node_modules";
    "vendor";
  ]

let normalize_relative path =
  String.map (fun character -> if character = '\\' then '/' else character) path

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let lowercase_extension path = Filename.extension path |> String.lowercase_ascii

let interpreter_for_extension = function
  | ".sh" -> Some "sh"
  | ".bash" -> Some "bash"
  | ".zsh" -> Some "zsh"
  | ".fish" -> Some "fish"
  | ".ps1" | ".psm1" -> Some "powershell"
  | ".cmd" | ".bat" -> Some "cmd"
  | ".nu" -> Some "nu"
  | _ -> None

let shell_extensions =
  [ ".sh"; ".bash"; ".zsh"; ".fish"; ".ps1"; ".psm1"; ".cmd"; ".bat"; ".nu" ]

let ignored_structured_filenames =
  [
    "package-lock.json";
    "npm-shrinkwrap.json";
    "pnpm-lock.yaml";
    "pnpm-lock.yml";
  ]

let split_words value =
  value |> String.split_on_char ' '
  |> List.concat_map (String.split_on_char '\t')
  |> List.filter (fun part -> part <> "")

let basename executable =
  let executable =
    String.map
      (fun character -> if character = '\\' then '/' else character)
      executable
  in
  match List.rev (String.split_on_char '/' executable) with
  | name :: _ -> String.lowercase_ascii name
  | [] -> String.lowercase_ascii executable

let interpreter_for_shebang content =
  match String.split_on_char '\n' content with
  | first :: _ when String.length first >= 2 && String.sub first 0 2 = "#!" ->
      let command =
        String.sub first 2 (String.length first - 2) |> String.trim
      in
      begin match split_words command with
      | executable :: _ when String.starts_with ~prefix:"[" executable -> None
      | [] -> None
      | executable :: rest when basename executable = "env" ->
          let rec program = function
            | [] -> None
            | "-S" :: remaining -> program remaining
            | option :: remaining
              when String.length option > 0 && option.[0] = '-' ->
                program remaining
            | value :: _ -> Some (basename value)
          in
          program rest
      | executable :: _ -> Some (basename executable)
      end
  | _ -> None

let is_shell_interpreter interpreter =
  List.mem
    (String.lowercase_ascii interpreter)
    [
      "ash";
      "bash";
      "csh";
      "cmd";
      "dash";
      "elvish";
      "fish";
      "ksh";
      "mksh";
      "nu";
      "nushell";
      "oil";
      "osh";
      "powershell";
      "pwsh";
      "sh";
      "tcsh";
      "xonsh";
      "yash";
      "zsh";
    ]

let strip_quotes value =
  let value = String.trim value in
  let length = String.length value in
  if length >= 2 then
    match (value.[0], value.[length - 1]) with
    | ('\'' | '"'), ('\'' | '"') -> String.sub value 1 (length - 2)
    | _ -> value
  else value

let looks_like_shell value =
  let value = strip_quotes value |> String.trim in
  let first =
    match split_words value with
    | command :: _ -> String.lowercase_ascii command
    | [] -> ""
  in
  List.mem first
    [
      "bash";
      "cat";
      "cd";
      "chmod";
      "cmd";
      "cp";
      "curl";
      "echo";
      "env";
      "exec";
      "fish";
      "git";
      "mkdir";
      "mv";
      "nu";
      "printf";
      "pwsh";
      "rm";
      "sh";
      "test";
      "wget";
      "zsh";
    ]
  || List.exists
       (fun marker ->
         let marker_length = String.length marker in
         let rec search index =
           index + marker_length <= String.length value
           && (String.sub value index marker_length = marker
              || search (index + 1))
         in
         marker_length > 0 && search 0)
       [ "&&"; "||"; "$("; "${"; " | "; " > " ]

let executable_field_name name =
  let name = String.trim name |> String.lowercase_ascii in
  List.mem name
    [
      "automation";
      "cmd";
      "command";
      "commands";
      "exec";
      "execute";
      "hook";
      "hooks";
      "run";
      "script";
      "scripts";
      "shell";
    ]
  || String.ends_with ~suffix:"command" name
  || String.ends_with ~suffix:"script" name
  || String.ends_with ~suffix:"hook" name

let finding ~path ~kind ?interpreter ?locator content =
  {
    path = normalize_relative path;
    kind;
    interpreter;
    locator;
    content_hash = Sha256.hex content;
    source = content;
  }

let strip_json_comments source =
  let length = String.length source in
  let buffer = Buffer.create length in
  let rec loop index state escaped =
    if index >= length then Buffer.contents buffer
    else
      let character = source.[index] in
      match state with
      | `String ->
          Buffer.add_char buffer character;
          if escaped then loop (index + 1) `String false
          else if character = '\\' then loop (index + 1) `String true
          else if character = '"' then loop (index + 1) `Normal false
          else loop (index + 1) `String false
      | `Line_comment ->
          if character = '\n' then begin
            Buffer.add_char buffer character;
            loop (index + 1) `Normal false
          end
          else begin
            Buffer.add_char buffer ' ';
            loop (index + 1) `Line_comment false
          end
      | `Block_comment ->
          if character = '*' && index + 1 < length && source.[index + 1] = '/'
          then begin
            Buffer.add_string buffer "  ";
            loop (index + 2) `Normal false
          end
          else begin
            Buffer.add_char buffer (if character = '\n' then '\n' else ' ');
            loop (index + 1) `Block_comment false
          end
      | `Normal ->
          if character = '"' then begin
            Buffer.add_char buffer character;
            loop (index + 1) `String false
          end
          else if
            character = '/' && index + 1 < length && source.[index + 1] = '/'
          then begin
            Buffer.add_string buffer "  ";
            loop (index + 2) `Line_comment false
          end
          else if
            character = '/' && index + 1 < length && source.[index + 1] = '*'
          then begin
            Buffer.add_string buffer "  ";
            loop (index + 2) `Block_comment false
          end
          else begin
            Buffer.add_char buffer character;
            loop (index + 1) `Normal false
          end
  in
  loop 0 `Normal false

let strip_trailing_json_commas source =
  let length = String.length source in
  let buffer = Buffer.create length in
  let rec next_non_space index =
    if index >= length then None
    else
      match source.[index] with
      | ' ' | '\t' | '\r' | '\n' -> next_non_space (index + 1)
      | character -> Some character
  in
  let rec loop index quoted escaped =
    if index >= length then Buffer.contents buffer
    else
      let character = source.[index] in
      if quoted then begin
        Buffer.add_char buffer character;
        if escaped then loop (index + 1) true false
        else if character = '\\' then loop (index + 1) true true
        else if character = '"' then loop (index + 1) false false
        else loop (index + 1) true false
      end
      else if character = '"' then begin
        Buffer.add_char buffer character;
        loop (index + 1) true false
      end
      else if character = ',' then
        begin match next_non_space (index + 1) with
        | Some (']' | '}') -> loop (index + 1) false false
        | _ ->
            Buffer.add_char buffer character;
            loop (index + 1) false false
        end
      else begin
        Buffer.add_char buffer character;
        loop (index + 1) false false
      end
  in
  loop 0 false false

let parse_json_relaxed source =
  source |> strip_json_comments |> strip_trailing_json_commas
  |> Yojson.Safe.from_string

let package_findings ~path content =
  try
    match Yojson.Safe.from_string content with
    | `Assoc fields ->
        begin match List.assoc_opt "scripts" fields with
        | Some (`Assoc scripts) ->
            List.filter_map
              (fun (name, value) ->
                match value with
                | `String script ->
                    Some
                      (finding ~path ~kind:Embedded_shell
                         ~interpreter:"package-shell"
                         ~locator:("scripts." ^ name) script)
                | _ -> None)
              scripts
        | _ -> []
        end
    | _ -> []
  with Yojson.Json_error _ -> []

let json_candidate_findings ~path content =
  let rec collect locator executable_context accumulator = function
    | `Assoc fields ->
        List.fold_left
          (fun accumulator (name, value) ->
            let child = String.concat "" [ locator; "."; name ] in
            collect child
              (executable_context || executable_field_name name)
              accumulator value)
          accumulator fields
    | `List values ->
        List.fold_left
          (fun accumulator (index, value) ->
            let child = Printf.sprintf "%s[%d]" locator index in
            collect child executable_context accumulator value)
          accumulator
          (List.mapi (fun index value -> (index, value)) values)
    | `String value when executable_context && looks_like_shell value ->
        finding ~path ~kind:Candidate ~locator value :: accumulator
    | `String _ | `Int _ | `Intlit _ | `Float _ | `Bool _ | `Null -> accumulator
    | `Tuple values ->
        collect locator executable_context accumulator (`List values)
    | `Variant (_, value) ->
        Option.fold ~none:accumulator
          ~some:(collect locator executable_context accumulator)
          value
  in
  try parse_json_relaxed content |> collect "$" false [] |> List.rev
  with Yojson.Json_error _ -> []

let vscode_task_findings ~path content =
  try
    match parse_json_relaxed content |> Yojson.Safe.Util.member "tasks" with
    | `List tasks ->
        tasks
        |> List.mapi (fun index task ->
            match task with
            | `Assoc fields ->
                begin match
                  (List.assoc_opt "type" fields, List.assoc_opt "command" fields)
                with
                | Some (`String kind), Some (`String command)
                  when String.lowercase_ascii kind = "shell" ->
                    let arguments =
                      match List.assoc_opt "args" fields with
                      | Some (`List values) ->
                          List.filter_map
                            (function `String value -> Some value | _ -> None)
                            values
                      | _ -> []
                    in
                    let interpreter =
                      match List.assoc_opt "options" fields with
                      | Some (`Assoc options) ->
                          begin match List.assoc_opt "shell" options with
                          | Some (`Assoc shell) ->
                              begin match List.assoc_opt "executable" shell with
                              | Some (`String executable) -> basename executable
                              | _ -> "vscode-shell"
                              end
                          | _ -> "vscode-shell"
                          end
                      | _ -> "vscode-shell"
                    in
                    let source = String.concat " " (command :: arguments) in
                    Some
                      (finding ~path ~kind:Embedded_shell ~interpreter
                         ~locator:(Printf.sprintf "tasks.%d.command" index)
                         source)
                | _ -> None
                end
            | _ -> None)
        |> List.filter_map Fun.id
    | _ -> []
  with Yojson.Json_error _ | Yojson.Safe.Util.Type_error _ -> []

let indexed_lines content =
  String.split_on_char '\n' content
  |> List.mapi (fun index line -> (index + 1, line))

let makefile_findings ~path content =
  indexed_lines content
  |> List.filter_map (fun (line_number, line) ->
      if String.length line > 0 && line.[0] = '\t' then
        Some
          (finding ~path ~kind:Embedded_shell ~interpreter:"sh"
             ~locator:(Printf.sprintf "recipe:%d" line_number)
             (String.sub line 1 (String.length line - 1)))
      else None)

let dockerfile_findings ~path content =
  let lines = indexed_lines content |> Array.of_list in
  let continuation value =
    let value = String.trim value in
    String.ends_with ~suffix:"\\" value
  in
  let without_continuation value =
    let value = String.trim value in
    String.sub value 0 (String.length value - 1) |> String.trim
  in
  let rec collect_continuation index parts =
    if index >= Array.length lines then (index, List.rev parts)
    else
      let _, line = lines.(index) in
      let line = String.trim line in
      if continuation line then
        collect_continuation (index + 1) (without_continuation line :: parts)
      else (index + 1, List.rev (line :: parts))
  in
  let rec loop index accumulator =
    if index >= Array.length lines then List.rev accumulator
    else
      let line_number, line = lines.(index) in
      let trimmed = String.trim line in
      if
        String.length trimmed >= 4
        && String.uppercase_ascii (String.sub trimmed 0 4) = "RUN "
      then
        let command = String.sub trimmed 4 (String.length trimmed - 4) in
        if String.starts_with ~prefix:"[" (String.trim command) then
          loop (index + 1) accumulator
        else
          let next, parts =
            if continuation command then
              collect_continuation (index + 1) [ without_continuation command ]
            else (index + 1, [ String.trim command ])
          in
          let command = String.concat " " parts in
          loop next
            (finding ~path ~kind:Embedded_shell ~interpreter:"sh"
               ~locator:(Printf.sprintf "RUN:%d" line_number)
               command
            :: accumulator)
      else loop (index + 1) accumulator
  in
  loop 0 []

let toml_candidate_findings ~path content =
  let strip_comment value =
    let length = String.length value in
    let rec loop index quote escaped =
      if index >= length then value
      else
        let character = value.[index] in
        match quote with
        | Some '"' when escaped -> loop (index + 1) quote false
        | Some '"' when character = '\\' -> loop (index + 1) quote true
        | Some delimiter when character = delimiter ->
            loop (index + 1) None false
        | Some _ -> loop (index + 1) quote false
        | None when character = '"' || character = '\'' ->
            loop (index + 1) (Some character) false
        | None when character = '#' -> String.sub value 0 index
        | None -> loop (index + 1) None false
    in
    loop 0 None false |> String.trim
  in
  let decode_string value =
    let length = String.length value in
    if length >= 2 && value.[0] = '\'' && value.[length - 1] = '\'' then
      Some (String.sub value 1 (length - 2))
    else
      try
        match Yojson.Safe.from_string value with
        | `String value -> Some value
        | _ -> None
      with Yojson.Json_error _ -> None
  in
  indexed_lines content
  |> List.filter_map (fun (line_number, line) ->
      match String.index_opt line '=' with
      | None -> None
      | Some separator ->
          let key =
            String.sub line 0 separator
            |> String.trim |> String.split_on_char '.' |> List.rev
            |> function
            | name :: _ -> strip_quotes name
            | [] -> ""
          in
          let value =
            String.sub line (separator + 1) (String.length line - separator - 1)
            |> String.trim
          in
          let value = strip_comment value in
          begin match decode_string value with
          | Some value when executable_field_name key && looks_like_shell value
            ->
              Some
                (finding ~path ~kind:Candidate
                   ~locator:(Printf.sprintf "line:%d" line_number)
                   value)
          | _ -> None
          end)

type pipeline = Github_actions | Gitlab_ci | Azure_pipelines | Circle_ci

let pipeline_for_path path =
  let lower = String.lowercase_ascii (normalize_relative path) in
  if String.starts_with ~prefix:".github/workflows/" lower then
    Some Github_actions
  else if
    String.starts_with ~prefix:".github/actions/" lower
    && (String.ends_with ~suffix:"/action.yml" lower
       || String.ends_with ~suffix:"/action.yaml" lower)
  then Some Github_actions
  else if lower = ".gitlab-ci.yml" || lower = ".gitlab-ci.yaml" then
    Some Gitlab_ci
  else if lower = "azure-pipelines.yml" || lower = "azure-pipelines.yaml" then
    Some Azure_pipelines
  else if String.starts_with ~prefix:".circleci/" lower then Some Circle_ci
  else None

let indentation line =
  let rec loop index =
    if index < String.length line && line.[index] = ' ' then loop (index + 1)
    else index
  in
  loop 0

let yaml_key value =
  let value = String.trim value in
  let value =
    if String.starts_with ~prefix:"-" value then
      String.sub value 1 (String.length value - 1) |> String.trim
    else value
  in
  String.lowercase_ascii value

let yaml_block_marker value =
  List.mem (String.trim value) [ "|"; "|-"; "|+"; ">"; ">-"; ">+" ]

let yaml_findings ~path ~pipeline content =
  let lines = indexed_lines content |> Array.of_list in
  let known = Option.is_some pipeline in
  let known_keys =
    match pipeline with
    | Some Github_actions -> [ "run" ]
    | Some Gitlab_ci -> [ "script"; "before_script"; "after_script" ]
    | Some Azure_pipelines -> [ "script"; "bash"; "pwsh"; "powershell" ]
    | Some Circle_ci -> [ "run"; "command" ]
    | None -> []
  in
  let sequence_keys =
    match pipeline with
    | Some Gitlab_ci -> [ "script"; "before_script"; "after_script" ]
    | _ -> []
  in
  let interpreter_for_key = function
    | "pwsh" | "powershell" -> "powershell"
    | "bash" -> "bash"
    | _ -> "sh"
  in
  let key_and_value line =
    match String.index_opt line ':' with
    | None -> None
    | Some separator ->
        let key = String.sub line 0 separator |> yaml_key in
        let value =
          String.sub line (separator + 1) (String.length line - separator - 1)
          |> strip_quotes
        in
        Some (key, value)
  in
  let semantic_indentation line =
    let trimmed = String.trim line in
    indentation line + if String.starts_with ~prefix:"-" trimmed then 2 else 0
  in
  let interpreter_for_shell value =
    match split_words (strip_quotes value) with
    | [] -> None
    | executable :: _ ->
        begin match basename executable with
        | "pwsh" | "pwsh.exe" | "powershell" | "powershell.exe" ->
            Some "powershell"
        | "cmd" | "cmd.exe" -> Some "cmd"
        | interpreter -> Some interpreter
        end
  in
  let github_shell_for index =
    let _, run_line = lines.(index) in
    let target_indentation = semantic_indentation run_line in
    let rec search direction cursor =
      if cursor < 0 || cursor >= Array.length lines then None
      else
        let _, line = lines.(cursor) in
        let trimmed = String.trim line in
        if trimmed = "" then search direction (cursor + direction)
        else
          let current_indentation = semantic_indentation line in
          if current_indentation < target_indentation then None
          else if current_indentation > target_indentation then
            search direction (cursor + direction)
          else
            match key_and_value line with
            | Some ("shell", value) -> interpreter_for_shell value
            | _ when String.starts_with ~prefix:"-" trimmed -> None
            | _ -> search direction (cursor + direction)
    in
    match search (-1) (index - 1) with
    | Some interpreter -> Some interpreter
    | None -> search 1 (index + 1)
  in
  let interpreter_for_location index key =
    match pipeline with
    | Some Github_actions -> Option.value ~default:"sh" (github_shell_for index)
    | _ -> interpreter_for_key key
  in
  let collect_block start base_indentation =
    let finish = ref start in
    while
      !finish < Array.length lines
      &&
      let _, line = lines.(!finish) in
      String.trim line = "" || indentation line > base_indentation
    do
      incr finish
    done;
    let block_lines =
      Array.sub lines start (!finish - start) |> Array.to_list |> List.map snd
    in
    let content_indentation =
      block_lines
      |> List.filter (fun line -> String.trim line <> "")
      |> List.map indentation
      |> function
      | [] -> base_indentation + 1
      | first :: rest -> List.fold_left min first rest
    in
    let block =
      block_lines
      |> List.map (fun line ->
          if String.trim line = "" then ""
          else
            String.sub line content_indentation
              (String.length line - content_indentation))
      |> String.concat "\n"
    in
    let block =
      if block = "" || String.ends_with ~suffix:"\n" block then block
      else block ^ "\n"
    in
    (block, !finish)
  in
  let collect_sequence start base_indentation key =
    let cursor = ref start in
    let findings = ref [] in
    while
      !cursor < Array.length lines
      &&
      let _, line = lines.(!cursor) in
      String.trim line = "" || indentation line > base_indentation
    do
      let line_number, line = lines.(!cursor) in
      let trimmed = String.trim line in
      if String.starts_with ~prefix:"-" trimmed then begin
        let value =
          String.sub trimmed 1 (String.length trimmed - 1)
          |> String.trim |> strip_quotes
        in
        if yaml_block_marker value then begin
          let block, next = collect_block (!cursor + 1) (indentation line) in
          if block <> "" then
            findings :=
              finding ~path ~kind:Embedded_shell ~interpreter:"sh"
                ~locator:(Printf.sprintf "%s:%d" key line_number)
                block
              :: !findings;
          cursor := next
        end
        else begin
          if value <> "" then
            findings :=
              finding ~path ~kind:Embedded_shell ~interpreter:"sh"
                ~locator:(Printf.sprintf "%s:%d" key line_number)
                value
              :: !findings;
          incr cursor
        end
      end
      else incr cursor
    done;
    (List.rev !findings, !cursor)
  in
  let rec loop index accumulator =
    if index >= Array.length lines then List.rev accumulator
    else
      let line_number, line = lines.(index) in
      match String.index_opt line ':' with
      | None -> loop (index + 1) accumulator
      | Some separator ->
          let key = String.sub line 0 separator |> yaml_key in
          let value =
            String.sub line (separator + 1) (String.length line - separator - 1)
            |> strip_quotes
          in
          if known && List.mem key sequence_keys && value = "" then
            let findings, next =
              collect_sequence (index + 1) (indentation line) key
            in
            let accumulator =
              List.fold_left
                (fun values value -> value :: values)
                accumulator findings
            in
            loop next accumulator
          else if known && List.mem key known_keys && yaml_block_marker value
          then
            let block, next = collect_block (index + 1) (indentation line) in
            let accumulator =
              if block = "" then accumulator
              else
                finding ~path ~kind:Embedded_shell
                  ~interpreter:(interpreter_for_location index key)
                  ~locator:(Printf.sprintf "%s:%d" key line_number)
                  block
                :: accumulator
            in
            loop next accumulator
          else if value = "" then loop (index + 1) accumulator
          else if known && List.mem key known_keys then
            loop (index + 1)
              (finding ~path ~kind:Embedded_shell
                 ~interpreter:(interpreter_for_location index key)
                 ~locator:(Printf.sprintf "%s:%d" key line_number)
                 value
              :: accumulator)
          else if known then loop (index + 1) accumulator
          else if
            (not (executable_field_name key)) || not (looks_like_shell value)
          then loop (index + 1) accumulator
          else
            loop (index + 1)
              (finding ~path ~kind:Candidate
                 ~locator:(Printf.sprintf "line:%d" line_number)
                 value
              :: accumulator)
  in
  loop 0 []

let findings_for_file ~relative ~absolute =
  try
    let metadata = Unix.lstat absolute in
    if
      metadata.Unix.st_kind <> Unix.S_REG
      || metadata.Unix.st_size > 4 * 1024 * 1024
    then []
    else
      let content = read_file absolute in
      let extension = lowercase_extension relative in
      let filename = Filename.basename relative |> String.lowercase_ascii in
      let normalized = normalize_relative relative |> String.lowercase_ascii in
      if List.mem filename ignored_structured_filenames then []
      else if List.mem extension shell_extensions then
        [
          finding ~path:relative ~kind:Shell_file
            ?interpreter:(interpreter_for_extension extension)
            content;
        ]
      else
        let shell_shebang =
          match interpreter_for_shebang content with
          | Some interpreter when is_shell_interpreter interpreter ->
              Some interpreter
          | Some _ | None -> None
        in
        match shell_shebang with
        | Some interpreter ->
            [ finding ~path:relative ~kind:Shell_file ~interpreter content ]
        | None when filename = "package.json" ->
            package_findings ~path:relative content
        | None when String.ends_with ~suffix:".vscode/tasks.json" normalized ->
            vscode_task_findings ~path:relative content
        | None
          when filename = "makefile" || filename = "gnumakefile"
               || extension = ".mk" ->
            makefile_findings ~path:relative content
        | None
          when filename = "dockerfile"
               || String.starts_with ~prefix:"dockerfile." filename
               || extension = ".dockerfile" ->
            dockerfile_findings ~path:relative content
        | None when extension = ".yaml" || extension = ".yml" ->
            yaml_findings ~path:relative
              ~pipeline:(pipeline_for_path relative)
              content
        | None when extension = ".json" ->
            json_candidate_findings ~path:relative content
        | None when extension = ".toml" ->
            toml_candidate_findings ~path:relative content
        | None ->
            Host_source.detect ~path:relative content
            |> List.map (fun (detection : Host_source.detection) ->
                finding ~path:relative
                  ~kind:
                    (match detection.kind with
                    | Host_source.Embedded -> Embedded_shell
                    | Host_source.Candidate -> Candidate)
                  ~interpreter:detection.interpreter ~locator:detection.locator
                  detection.source)
  with Sys_error _ | Unix.Unix_error _ -> []

let compare_findings left right =
  match String.compare left.path right.path with
  | 0 -> compare left.locator right.locator
  | value -> value

let read_channel channel =
  let buffer = Buffer.create 4096 in
  let chunk = Bytes.create 4096 in
  let rec loop () =
    match input channel chunk 0 (Bytes.length chunk) with
    | 0 -> Buffer.contents buffer
    | count ->
        Buffer.add_subbytes buffer chunk 0 count;
        loop ()
  in
  loop ()

let git_marker_is_valid root =
  let marker = Filename.concat root ".git" in
  try
    if Sys.is_directory marker then
      Sys.file_exists (Filename.concat marker "HEAD")
    else Sys.file_exists marker
  with Sys_error _ -> false

let contains_ignored_directory relative =
  normalize_relative relative
  |> String.split_on_char '/'
  |> List.exists (fun component -> List.mem component ignored_directories)

let git_inventory root =
  if not (git_marker_is_valid root) then None
  else
    try
      let arguments =
        [|
          "git";
          "-C";
          root;
          "ls-files";
          "--cached";
          "--others";
          "--exclude-standard";
          "-z";
          "--";
        |]
      in
      let channel = Unix.open_process_args_in "git" arguments in
      let output = read_channel channel in
      match Unix.close_process_in channel with
      | Unix.WEXITED 0 ->
          Some
            (output
            |> String.split_on_char '\000'
            |> List.filter (fun relative ->
                relative <> "" && not (contains_ignored_directory relative))
            |> List.sort String.compare)
      | Unix.WEXITED _ | Unix.WSIGNALED _ | Unix.WSTOPPED _ -> None
    with Sys_error _ | Unix.Unix_error _ -> None

let scan ~root =
  let rec walk relative absolute accumulator =
    let entries =
      try Sys.readdir absolute |> Array.to_list |> List.sort String.compare
      with _ -> []
    in
    List.fold_left
      (fun current name ->
        let child_absolute = Filename.concat absolute name in
        let child_relative =
          if relative = "" then name else Filename.concat relative name
        in
        try
          match (Unix.lstat child_absolute).Unix.st_kind with
          | Unix.S_DIR when List.mem name ignored_directories -> current
          | Unix.S_DIR -> walk child_relative child_absolute current
          | Unix.S_REG ->
              List.rev_append
                (findings_for_file
                   ~relative:(normalize_relative child_relative)
                   ~absolute:child_absolute)
                current
          | Unix.S_LNK | Unix.S_CHR | Unix.S_BLK | Unix.S_FIFO | Unix.S_SOCK ->
              current
        with Unix.Unix_error _ -> current)
      accumulator entries
  in
  let findings =
    match git_inventory root with
    | Some files ->
        List.fold_left
          (fun accumulator relative ->
            List.rev_append
              (findings_for_file ~relative
                 ~absolute:(Filename.concat root relative))
              accumulator)
          [] files
    | None -> walk "" root []
  in
  List.sort compare_findings findings

let kind_to_string = function
  | Shell_file -> "shell_file"
  | Embedded_shell -> "embedded_shell"
  | Candidate -> "candidate"

let to_yojson finding =
  `Assoc
    [
      ("path", `String finding.path);
      ("kind", `String (kind_to_string finding.kind));
      ( "interpreter",
        Option.fold ~none:`Null
          ~some:(fun value -> `String value)
          finding.interpreter );
      ( "locator",
        Option.fold ~none:`Null
          ~some:(fun value -> `String value)
          finding.locator );
      ("content_hash", `String finding.content_hash);
    ]
