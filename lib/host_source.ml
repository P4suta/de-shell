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
               always "execSync(";
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

let find_marker line marker =
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
  if marker = "" then None else loop 0

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

let javascript_imports_exec content =
  let compact = compact content in
  List.exists
    (fun binding -> contains ~needle:binding compact)
    [
      "import{exec}from\"node:child_process\"";
      "import{exec}from'node:child_process'";
      "import{exec}from\"child_process\"";
      "import{exec}from'child_process'";
      "const{exec}=require(\"node:child_process\")";
      "const{exec}=require('node:child_process')";
      "const{exec}=require(\"child_process\")";
      "const{exec}=require('child_process')";
      "let{exec}=require(\"node:child_process\")";
      "let{exec}=require('node:child_process')";
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

let direct_detection config line_number line masked =
  let rec find = function
    | [] -> None
    | rule :: rest ->
        begin match find_marker masked rule.marker with
        | Some index when condition_matches masked rule.condition ->
            let command_start = index + String.length rule.marker in
            let kind, source =
              match first_literal_after line command_start with
              | Some literal
                when not
                       (host_interpolates config line literal
                       || dynamic_operator_after masked literal.finish) ->
                  (Embedded, literal.value)
              | Some _ -> (Candidate, candidate_source line)
              | None -> (Candidate, candidate_source line)
            in
            Some
              {
                kind;
                interpreter = "platform-shell";
                locator =
                  Printf.sprintf "source:%s:%d" config.language line_number;
                source;
              }
        | Some _ | None -> find rest
        end
  in
  find config.direct_rules

let argv_detection config line_number line masked =
  let detect marker anywhere =
    match find_marker masked marker with
    | None -> None
    | Some index ->
        let start = if anywhere then 0 else index + String.length marker in
        let literals = extract_literals line start in
        let values = List.map (fun literal -> literal.value) literals in
        begin match recognize_argv values with
        | No_shell -> None
        | Dynamic interpreter ->
            Some
              {
                kind = Candidate;
                interpreter;
                locator =
                  Printf.sprintf "source:%s:%d" config.language line_number;
                source = candidate_source line;
              }
        | Static (interpreter, _, consumed)
          when let command_literal = List.nth literals (consumed - 1) in
               let after_command = skip_space masked command_literal.finish in
               List.length literals > consumed
               || after_command < String.length masked
                  && masked.[after_command] = ','
               || List.exists (host_interpolates config line) literals
               || dynamic_operator_after masked start ->
            Some
              {
                kind = Candidate;
                interpreter;
                locator =
                  Printf.sprintf "source:%s:%d" config.language line_number;
                source = candidate_source line;
              }
        | Static (interpreter, source, _) ->
            Some
              {
                kind = Embedded;
                interpreter;
                locator =
                  Printf.sprintf "source:%s:%d" config.language line_number;
                source;
              }
        end
  in
  let rec first anywhere = function
    | [] -> None
    | marker :: rest ->
        begin match detect marker anywhere with
        | Some _ as detection -> detection
        | None -> first anywhere rest
        end
  in
  match first false config.argv_markers with
  | Some _ as detection -> detection
  | None -> first true config.argv_anywhere_markers

let detect ~path content =
  match config_for_extension (Filename.extension path) with
  | None -> []
  | Some config ->
      let config =
        if config.language = "javascript" && javascript_imports_exec content
        then
          {
            config with
            direct_rules = config.direct_rules @ [ always "exec(" ];
          }
        else config
      in
      let _, detections =
        content |> String.split_on_char '\n'
        |> List.mapi (fun index line -> (index + 1, line))
        |> List.fold_left
             (fun (in_block, detections) (line_number, line) ->
               let masked, in_block = mask_line config in_block line in
               let detection =
                 match direct_detection config line_number line masked with
                 | Some _ as detection -> detection
                 | None -> argv_detection config line_number line masked
               in
               ( in_block,
                 Option.fold ~none:detections
                   ~some:(fun value -> value :: detections)
                   detection ))
             (false, [])
      in
      List.rev detections
