type kind = Embedded | Candidate

type detection = {
  kind : kind;
  interpreter : string;
  locator : string;
  source : string;
}

type condition = Always | Compact_contains of string
type direct_rule = { marker : string; condition : condition }

type config = {
  language : string;
  line_comments : string list;
  block_comment : (string * string) option;
  direct_rules : direct_rule list;
  argv_markers : string list;
  argv_anywhere_markers : string list;
}

let always marker = { marker; condition = Always }

let when_compact_contains marker value =
  { marker; condition = Compact_contains value }

let config ?(line_comments = []) ?block_comment ?(direct_rules = [])
    ?(argv_markers = []) ?(argv_anywhere_markers = []) language =
  {
    language;
    line_comments;
    block_comment;
    direct_rules;
    argv_markers;
    argv_anywhere_markers;
  }

let config_for_extension extension =
  match String.lowercase_ascii extension with
  | ".java" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~argv_markers:[ "ProcessBuilder(" ] "java")
  | ".kt" | ".kts" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~argv_markers:[ "ProcessBuilder(" ] "kotlin")
  | ".scala" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~argv_markers:[ "ProcessBuilder(" ] "scala")
  | ".groovy" | ".gradle" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~argv_anywhere_markers:[ ".execute(" ] "groovy")
  | ".py" ->
      Some
        (config ~line_comments:[ "#" ]
           ~direct_rules:
             [
               always "os.system(";
               always "os.popen(";
               when_compact_contains "subprocess.run(" "shell=True";
               when_compact_contains "subprocess.call(" "shell=True";
               when_compact_contains "subprocess.check_call(" "shell=True";
               when_compact_contains "subprocess.check_output(" "shell=True";
               when_compact_contains "subprocess.Popen(" "shell=True";
             ]
           "python")
  | ".js" | ".jsx" | ".mjs" | ".cjs" | ".ts" | ".tsx" | ".mts" | ".cts" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~direct_rules:
             [
               always "child_process.exec(";
               always "childProcess.exec(";
               always "child_process.execSync(";
               always "childProcess.execSync(";
             ]
           "javascript")
  | ".go" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~argv_markers:[ "exec.Command("; "exec.CommandContext(" ]
           "go")
  | ".rs" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~argv_markers:[ "Command::new(" ] "rust")
  | ".c" | ".h" | ".cc" | ".cpp" | ".cxx" | ".hpp" | ".m" | ".mm" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~direct_rules:[ always "system("; always "popen(" ]
           "c-family")
  | ".cs" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~argv_markers:[ "Process.Start(" ] "csharp")
  | ".fs" | ".fsx" | ".fsi" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("(*", "*)")
           ~argv_markers:[ "Process.Start(" ] "fsharp")
  | ".vb" ->
      Some
        (config ~line_comments:[ "'" ] ~argv_markers:[ "Process.Start(" ] "vb")
  | ".ml" | ".mli" ->
      Some
        (config ~block_comment:("(*", "*)")
           ~direct_rules:
             [
               always "Sys.command";
               always "Unix.system";
               always "Unix.open_process_in";
               always "Unix.open_process_out";
               always "Unix.open_process_full";
             ]
           "ocaml")
  | ".hs" | ".lhs" ->
      Some
        (config ~line_comments:[ "--" ] ~block_comment:("{-", "-}")
           ~direct_rules:[ always "callCommand"; always "shell" ]
           "haskell")
  | ".ex" | ".exs" ->
      Some
        (config ~line_comments:[ "#" ]
           ~direct_rules:[ always "System.shell(" ]
           ~argv_markers:[ "System.cmd(" ] "elixir")
  | ".erl" | ".hrl" ->
      Some
        (config ~line_comments:[ "%" ]
           ~direct_rules:[ always "os:cmd(" ]
           "erlang")
  | ".lua" ->
      Some
        (config ~line_comments:[ "--" ]
           ~direct_rules:[ always "os.execute("; always "io.popen(" ]
           "lua")
  | ".pl" | ".pm" ->
      Some
        (config ~line_comments:[ "#" ]
           ~direct_rules:[ always "system("; always "qx(" ]
           "perl")
  | ".rb" ->
      Some
        (config ~line_comments:[ "#" ]
           ~direct_rules:[ always "system("; always "exec(" ]
           "ruby")
  | ".php" ->
      Some
        (config ~line_comments:[ "//"; "#" ] ~block_comment:("/*", "*/")
           ~direct_rules:
             [
               always "shell_exec(";
               always "passthru(";
               always "system(";
               always "popen(";
               always "exec(";
             ]
           "php")
  | ".r" ->
      Some
        (config ~line_comments:[ "#" ]
           ~direct_rules:[ always "system("; always "shell(" ]
           "r")
  | ".nim" ->
      Some
        (config ~line_comments:[ "#" ]
           ~direct_rules:[ always "execShellCmd(" ]
           "nim")
  | ".d" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~direct_rules:[ always "executeShell(" ]
           "d")
  | ".clj" | ".cljs" | ".cljc" | ".edn" ->
      Some
        (config ~line_comments:[ ";" ]
           ~argv_markers:[ "shell/sh"; "clojure.java.shell/sh" ]
           "clojure")
  | ".dart" ->
      Some
        (config ~line_comments:[ "//" ] ~block_comment:("/*", "*/")
           ~argv_markers:[ "Process.run("; "Process.start(" ]
           "dart")
  | ".jl" ->
      Some (config ~line_comments:[ "#" ] ~argv_markers:[ "Cmd(" ] "julia")
  | ".zig" ->
      Some
        (config ~line_comments:[ "//" ]
           ~argv_markers:[ "std.process.Child.run(" ]
           "zig")
  | ".cr" ->
      Some
        (config ~line_comments:[ "#" ] ~argv_markers:[ "Process.run(" ]
           "crystal")
  | _ -> None

let starts_at value index needle =
  let needle_length = String.length needle in
  index >= 0
  && index + needle_length <= String.length value
  && String.sub value index needle_length = needle

let mask_line config in_block line =
  let masked = Bytes.of_string line in
  let blank index count =
    for cursor = index to index + count - 1 do
      Bytes.set masked cursor ' '
    done
  in
  let line_comment_at index =
    List.find_opt (starts_at line index) config.line_comments
  in
  let rec loop index in_block quote escaped =
    if index >= String.length line then (Bytes.to_string masked, in_block)
    else
      match (in_block, config.block_comment) with
      | true, Some (_, finish) when starts_at line index finish ->
          blank index (String.length finish);
          loop (index + String.length finish) false quote false
      | true, Some _ ->
          Bytes.set masked index ' ';
          loop (index + 1) true quote false
      | true, None -> (Bytes.to_string masked, false)
      | false, _ ->
          begin match quote with
          | Some delimiter ->
              Bytes.set masked index ' ';
              if escaped then loop (index + 1) false quote false
              else if line.[index] = '\\' then loop (index + 1) false quote true
              else if line.[index] = delimiter then
                loop (index + 1) false None false
              else loop (index + 1) false quote false
          | None ->
              begin match line_comment_at index with
              | Some marker ->
                  blank index (String.length line - index);
                  (Bytes.to_string masked, false)
              | None ->
                  begin match config.block_comment with
                  | Some (start, _) when starts_at line index start ->
                      blank index (String.length start);
                      loop (index + String.length start) true None false
                  | _ ->
                      begin match line.[index] with
                      | ('\'' | '"' | '`') as delimiter ->
                          Bytes.set masked index ' ';
                          loop (index + 1) false (Some delimiter) false
                      | _ -> loop (index + 1) false None false
                      end
                  end
              end
          end
  in
  loop 0 in_block None false

let identifier_character = function
  | 'a' .. 'z' | 'A' .. 'Z' | '0' .. '9' | '_' -> true
  | _ -> false

let find_marker_from line marker start =
  let marker_length = String.length marker in
  let rec loop index =
    if index + marker_length > String.length line then None
    else if starts_at line index marker then
      let valid_boundary =
        index = 0
        || (not (identifier_character line.[index - 1]))
        || not (identifier_character marker.[0])
      in
      if valid_boundary then Some index else loop (index + 1)
    else loop (index + 1)
  in
  if marker = "" then None else loop start

let find_marker line marker = find_marker_from line marker 0

let compact value =
  value |> String.to_seq
  |> Seq.filter (function ' ' | '\t' | '\r' | '\n' -> false | _ -> true)
  |> String.of_seq

let condition_matches masked = function
  | Always -> true
  | Compact_contains expected ->
      Option.is_some (find_marker (compact masked) expected)

let contains ~needle value =
  let rec loop index =
    if index + String.length needle > String.length value then false
    else if starts_at value index needle then true
    else loop (index + 1)
  in
  needle = "" || loop 0

let javascript_imports_binding content binding =
  let compact = compact content in
  let instantiate pattern =
    match String.index_opt pattern '%' with
    | None -> pattern
    | Some index ->
        String.sub pattern 0 index ^ binding
        ^ String.sub pattern (index + 2) (String.length pattern - index - 2)
  in
  List.exists
    (fun pattern -> contains ~needle:(instantiate pattern) compact)
    [
      "import{%s}from\"node:child_process\"";
      "import{%s}from'node:child_process'";
      "import{%s}from\"child_process\"";
      "import{%s}from'child_process'";
      "const{%s}=require(\"node:child_process\")";
      "const{%s}=require('node:child_process')";
      "const{%s}=require(\"child_process\")";
      "const{%s}=require('child_process')";
      "let{%s}=require(\"node:child_process\")";
      "let{%s}=require('node:child_process')";
    ]

type literal = { value : string; start : int; finish : int; delimiter : char }

let literal_at line start =
  if start >= String.length line then None
  else
    match line.[start] with
    | ('\'' | '"' | '`') as delimiter ->
        let buffer = Buffer.create 32 in
        let rec loop index escaped =
          if index >= String.length line then None
          else
            let character = line.[index] in
            if escaped then begin
              Buffer.add_char buffer
                (match character with
                | 'n' -> '\n'
                | 'r' -> '\r'
                | 't' -> '\t'
                | value -> value);
              loop (index + 1) false
            end
            else if character = '\\' then loop (index + 1) true
            else if character = delimiter then
              Some
                {
                  value = Buffer.contents buffer;
                  start;
                  finish = index + 1;
                  delimiter;
                }
            else begin
              Buffer.add_char buffer character;
              loop (index + 1) false
            end
        in
        loop (start + 1) false
    | _ -> None

let skip_space line start =
  let rec loop index =
    if index < String.length line then
      match line.[index] with ' ' | '\t' -> loop (index + 1) | _ -> index
    else index
  in
  loop start

let first_literal_after line start = literal_at line (skip_space line start)

let extract_literals line start =
  let rec loop index accumulator =
    if index >= String.length line then List.rev accumulator
    else
      match line.[index] with
      | '\'' | '"' | '`' ->
          begin match literal_at line index with
          | Some literal -> loop literal.finish (literal :: accumulator)
          | None -> List.rev accumulator
          end
      | _ -> loop (index + 1) accumulator
  in
  loop start []

let basename executable =
  let normalized =
    String.map
      (fun character -> if character = '\\' then '/' else character)
      executable
  in
  match List.rev (String.split_on_char '/' normalized) with
  | name :: _ -> String.lowercase_ascii name
  | [] -> String.lowercase_ascii executable

let launcher_interpreter executable =
  match basename executable with
  | ("sh" | "dash" | "ksh" | "bash" | "zsh" | "fish") as shell -> Some shell
  | "nu" | "nushell" -> Some "nu"
  | "pwsh" | "pwsh.exe" | "powershell" | "powershell.exe" -> Some "powershell"
  | "cmd" | "cmd.exe" -> Some "cmd"
  | _ -> None

let command_flag interpreter =
  match interpreter with
  | "cmd" -> [ "/c" ]
  | "powershell" -> [ "-command"; "-c" ]
  | _ -> [ "-c" ]

type argv_result =
  | No_shell
  | Dynamic of string
  | Static of string * string * int

let command_after_combined_flag interpreter argument =
  let lower = String.lowercase_ascii argument in
  command_flag interpreter
  |> List.find_map (fun flag ->
      let prefix = flag ^ " " in
      if String.starts_with ~prefix lower then
        Some
          (String.sub argument (String.length prefix)
             (String.length argument - String.length prefix)
          |> String.trim)
      else None)

let recognize_argv literals =
  let rec loop skipped = function
    | [] -> No_shell
    | executable :: rest ->
        begin match launcher_interpreter executable with
        | None -> loop (skipped + 1) rest
        | Some interpreter ->
            begin match rest with
            | argument :: command :: _
              when List.mem
                     (String.lowercase_ascii argument)
                     (command_flag interpreter) ->
                Static (interpreter, command, skipped + 3)
            | argument :: _ ->
                begin match
                  command_after_combined_flag interpreter argument
                with
                | Some command when command <> "" ->
                    Static (interpreter, command, skipped + 2)
                | Some _ -> Dynamic interpreter
                | None
                  when List.mem
                         (String.lowercase_ascii argument)
                         (command_flag interpreter) ->
                    Dynamic interpreter
                | None -> loop (skipped + 1) rest
                end
            | [] -> Dynamic interpreter
            end
        end
  in
  loop 0 literals

let candidate_source line = String.trim line

let dynamic_operator_after masked finish =
  let suffix =
    if finish >= String.length masked then ""
    else String.sub masked finish (String.length masked - finish)
  in
  List.exists
    (fun marker -> contains ~needle:marker suffix)
    [ "++"; "+"; "^"; ".."; ".format("; ".formatted(" ]

let host_interpolates config line literal =
  match config.language with
  | "javascript" ->
      literal.delimiter = '`' && contains ~needle:"${" literal.value
  | "kotlin" | "groovy" -> contains ~needle:"$" literal.value
  | "ruby" | "elixir" | "crystal" -> contains ~needle:"#{" literal.value
  | "php" | "perl" | "julia" -> contains ~needle:"$" literal.value
  | "csharp" | "fsharp" -> literal.start > 0 && line.[literal.start - 1] = '$'
  | _ -> false

let source_locator config line_number column =
  Printf.sprintf "source:%s:%08d:%08d" config.language line_number column

let direct_rule_allowed config masked rule index =
  let member_receiver =
    index > 0 && (masked.[index - 1] = '.' || masked.[index - 1] = '>')
  in
  if
    config.language = "javascript"
    && (rule.marker = "exec(" || rule.marker = "execSync(")
  then not member_receiver
  else if config.language = "c-family" || config.language = "php" then
    not member_receiver
  else true

let direct_detections config line_number line masked =
  let statement_start index =
    let rec loop cursor =
      if cursor <= 0 then 0
      else if masked.[cursor - 1] = ';' then cursor
      else loop (cursor - 1)
    in
    loop index
  in
  let statement_finish index =
    match String.index_from_opt masked index ';' with
    | Some separator -> separator
    | None -> String.length masked
  in
  let detections =
    List.concat_map
      (fun rule ->
        let rec find start accumulator =
          match find_marker_from masked rule.marker start with
          | Some index when direct_rule_allowed config masked rule index ->
              let statement_start = statement_start index in
              let statement_finish = statement_finish index in
              let statement_length = statement_finish - statement_start in
              let statement_line =
                String.sub line statement_start statement_length
              in
              let statement_masked =
                String.sub masked statement_start statement_length
              in
              if not (condition_matches statement_masked rule.condition) then
                find (index + String.length rule.marker) accumulator
              else
                let command_start = index + String.length rule.marker in
                let kind, source =
                  match first_literal_after line command_start with
                  | Some literal
                    when not
                           (host_interpolates config line literal
                           || dynamic_operator_after
                                (String.sub masked 0 statement_finish)
                                literal.finish) ->
                      (Embedded, literal.value)
                  | Some _ -> (Candidate, candidate_source statement_line)
                  | None -> (Candidate, candidate_source statement_line)
                in
                let detection =
                  {
                    kind;
                    interpreter = "platform-shell";
                    locator = source_locator config line_number (index + 1);
                    source;
                  }
                in
                find
                  (index + String.length rule.marker)
                  ((index, detection) :: accumulator)
          | Some index -> find (index + String.length rule.marker) accumulator
          | None -> List.rev accumulator
        in
        find 0 [])
      config.direct_rules
  in
  detections
  |> List.sort (fun (left, _) (right, _) -> Int.compare left right)
  |> List.map snd

let argv_detections config line_number line masked =
  let statement_start index =
    let rec loop cursor =
      if cursor <= 0 then 0
      else if masked.[cursor - 1] = ';' then cursor
      else loop (cursor - 1)
    in
    loop index
  in
  let statement_finish index =
    match String.index_from_opt masked index ';' with
    | Some separator -> separator
    | None -> String.length masked
  in
  let detect marker anywhere index =
    let segment_start = if anywhere then statement_start index else index in
    let segment_finish = statement_finish index in
    let segment_length = segment_finish - segment_start in
    let segment_line = String.sub line segment_start segment_length in
    let segment_masked = String.sub masked segment_start segment_length in
    let start =
      if anywhere then 0 else index + String.length marker - segment_start
    in
    let literals = extract_literals segment_line start in
    let values = List.map (fun literal -> literal.value) literals in
    begin match recognize_argv values with
    | No_shell -> None
    | Dynamic interpreter ->
        Some
          {
            kind = Candidate;
            interpreter;
            locator = source_locator config line_number (index + 1);
            source = candidate_source segment_line;
          }
    | Static (interpreter, _, consumed)
      when let command_literal = List.nth literals (consumed - 1) in
           let after_command =
             skip_space segment_masked command_literal.finish
           in
           List.length literals > consumed
           || after_command < String.length segment_masked
              && segment_masked.[after_command] = ','
           || List.exists (host_interpolates config segment_line) literals
           || dynamic_operator_after segment_masked start ->
        Some
          {
            kind = Candidate;
            interpreter;
            locator = source_locator config line_number (index + 1);
            source = candidate_source segment_line;
          }
    | Static (interpreter, source, _) ->
        Some
          {
            kind = Embedded;
            interpreter;
            locator = source_locator config line_number (index + 1);
            source;
          }
    end
  in
  let for_marker anywhere marker =
    let rec loop start accumulator =
      match find_marker_from masked marker start with
      | None -> List.rev accumulator
      | Some index ->
          let accumulator =
            match detect marker anywhere index with
            | None -> accumulator
            | Some detection -> (index, detection) :: accumulator
          in
          loop (index + String.length marker) accumulator
    in
    loop 0 []
  in
  List.concat_map (for_marker false) config.argv_markers
  @ List.concat_map (for_marker true) config.argv_anywhere_markers
  |> List.sort (fun (left, _) (right, _) -> Int.compare left right)
  |> List.map snd

let detect ~path content =
  match config_for_extension (Filename.extension path) with
  | None -> []
  | Some config ->
      let config =
        if config.language = "javascript" then
          let imported =
            [ "exec"; "execSync" ]
            |> List.filter (javascript_imports_binding content)
            |> List.map (fun binding -> always (binding ^ "("))
          in
          { config with direct_rules = config.direct_rules @ imported }
        else config
      in
      let _, detections =
        content |> String.split_on_char '\n'
        |> List.mapi (fun index line -> (index + 1, line))
        |> List.fold_left
             (fun (in_block, detections) (line_number, line) ->
               let masked, in_block = mask_line config in_block line in
               let line_detections =
                 direct_detections config line_number line masked
                 @ argv_detections config line_number line masked
                 |> List.sort (fun left right ->
                     String.compare left.locator right.locator)
               in
               (in_block, List.rev_append line_detections detections))
             (false, [])
      in
      List.rev detections
