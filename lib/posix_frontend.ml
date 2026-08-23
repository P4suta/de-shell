type severity = Warning | Diagnostic_error

type diagnostic = {
  severity : severity;
  message : string;
  span : Ir.source_span option;
}

type result = { root : Ir.node; diagnostics : diagnostic list }
type token = { text : string; dynamic : bool; start_byte : int; end_byte : int }
type item = Word of token | Pipe of int | Separator of int

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
  | name :: _ -> name
  | [] -> executable

let normalize_interpreter value =
  let value = basename value |> String.lowercase_ascii in
  if Filename.check_suffix value ".exe" then
    String.sub value 0 (String.length value - 4)
  else value

let interpreter_of_source source =
  match String.split_on_char '\n' source with
  | first :: _ when String.length first >= 2 && String.sub first 0 2 = "#!" ->
      let command =
        String.sub first 2 (String.length first - 2) |> String.trim
      in
      begin match split_words command with
      | [] -> "sh"
      | executable :: rest when normalize_interpreter executable = "env" ->
          let rec first_program = function
            | [] -> "sh"
            | "-S" :: remaining -> first_program remaining
            | option :: remaining
              when String.length option > 0 && option.[0] = '-' ->
                first_program remaining
            | program :: _ -> normalize_interpreter program
          in
          first_program rest
      | executable :: _ -> normalize_interpreter executable
      end
  | _ -> "sh"

let position_at source offset =
  let line = ref 1 in
  let column = ref 0 in
  for index = 0 to min offset (String.length source) - 1 do
    if source.[index] = '\n' then begin
      incr line;
      column := 0
    end
    else incr column
  done;
  (!line, !column)

let span_for_source ~path source =
  let end_line, end_column = position_at source (String.length source) in
  Ir.
    {
      file = path;
      start_line = 1;
      start_column = 0;
      end_line;
      end_column;
      start_byte = 0;
      end_byte = String.length source;
    }

let span_for_range ~path source ~start_byte ~end_byte =
  let start_line, start_column = position_at source start_byte in
  let end_line, end_column = position_at source end_byte in
  Ir.
    {
      file = path;
      start_line;
      start_column;
      end_line;
      end_column;
      start_byte;
      end_byte;
    }

let cover_spans left right =
  Ir.
    {
      file = left.file;
      start_line = left.start_line;
      start_column = left.start_column;
      end_line = right.end_line;
      end_column = right.end_column;
      start_byte = left.start_byte;
      end_byte = right.end_byte;
    }

let lex source =
  let length = String.length source in
  let items = ref [] in
  let buffer = Buffer.create 32 in
  let started = ref false in
  let word_start = ref 0 in
  let dynamic = ref false in
  let state = ref `Normal in
  let index = ref 0 in
  let failure = ref None in
  let flush_word end_byte =
    if !started then begin
      items :=
        Word
          {
            text = Buffer.contents buffer;
            dynamic = !dynamic;
            start_byte = !word_start;
            end_byte;
          }
        :: !items;
      Buffer.clear buffer;
      started := false;
      dynamic := false
    end
  in
  let start_word offset =
    if not !started then begin
      started := true;
      word_start := offset
    end
  in
  let add character =
    start_word !index;
    Buffer.add_char buffer character
  in
  while !index < length && !failure = None do
    let character = source.[!index] in
    begin match !state with
    | `Single -> if character = '\'' then state := `Normal else add character
    | `Double ->
        if character = '"' then state := `Normal
        else if character = '\\' then
          if !index + 1 >= length then
            failure := Some "trailing escape in double-quoted string"
          else begin
            let escaped = source.[!index + 1] in
            if List.mem escaped [ '$'; '`'; '"'; '\\'; '\n' ] then begin
              incr index;
              if escaped <> '\n' then add escaped
            end
            else add character
          end
        else begin
          if character = '$' || character = '`' then dynamic := true;
          add character
        end
    | `Normal ->
        begin match character with
        | ' ' | '\t' | '\r' -> flush_word !index
        | '\n' | ';' ->
            flush_word !index;
            items := Separator !index :: !items
        | '\'' ->
            start_word !index;
            state := `Single
        | '"' ->
            start_word !index;
            state := `Double
        | '\\' ->
            if !index + 1 >= length then failure := Some "trailing escape"
            else begin
              start_word !index;
              incr index;
              if source.[!index] <> '\n' then
                Buffer.add_char buffer source.[!index]
            end
        | '#' when not !started ->
            while !index < length && source.[!index] <> '\n' do
              incr index
            done;
            if !index < length then decr index else index := length - 1
        | '|' ->
            flush_word !index;
            if !index + 1 < length && source.[!index + 1] = '|' then
              failure :=
                Some "conditional shell operators are not in the static subset"
            else items := Pipe !index :: !items
        | '&' | '<' | '>' ->
            failure :=
              Some
                "redirection and asynchronous shell operators require a \
                 residual capsule"
        | '$' | '`' | '*' | '?' | '[' | ']' | '{' | '}' ->
            dynamic := true;
            add character
        | _ -> add character
        end
    end;
    incr index
  done;
  match (!failure, !state) with
  | Some message, _ -> Error message
  | None, (`Single | `Double) -> Error "unterminated quoted string"
  | None, `Normal ->
      flush_word length;
      Ok (List.rev !items)

let parse_items items =
  let groups = ref [] in
  let stages = ref [] in
  let words = ref [] in
  let after_pipe = ref false in
  let failure = ref None in
  let flush_stage () =
    match List.rev !words with
    | [] -> false
    | values ->
        stages := values :: !stages;
        words := [];
        true
  in
  let flush_group () =
    ignore (flush_stage ());
    match List.rev !stages with
    | [] -> ()
    | values ->
        groups := values :: !groups;
        stages := []
  in
  List.iter
    (function
      | Word value ->
          words := value :: !words;
          after_pipe := false
      | Pipe _ ->
          if not (flush_stage ()) then
            failure := Some "pipeline is missing a command";
          after_pipe := true
      | Separator _ ->
          if !after_pipe then
            failure := Some "pipeline is missing its final command"
          else flush_group ())
    items;
  if !after_pipe then failure := Some "pipeline is missing its final command";
  flush_group ();
  match !failure with
  | Some message -> Error message
  | None -> Ok (List.rev !groups)

let valid_environment_name name =
  let valid_first = function
    | 'A' .. 'Z' | 'a' .. 'z' | '_' -> true
    | _ -> false
  in
  let valid_rest = function
    | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> true
    | _ -> false
  in
  String.length name > 0
  && valid_first name.[0]
  && String.for_all valid_rest name

let assignment token =
  match String.index_opt token.text '=' with
  | None -> None
  | Some index ->
      let name = String.sub token.text 0 index in
      if valid_environment_name name then
        Some
          ( name,
            String.sub token.text (index + 1)
              (String.length token.text - index - 1) )
      else None

let unsupported_commands =
  [
    "!";
    ".";
    "alias";
    "break";
    "builtin";
    "case";
    "cd";
    "command";
    "continue";
    "declare";
    "dirs";
    "do";
    "done";
    "elif";
    "else";
    "enable";
    "esac";
    "eval";
    "exec";
    "exit";
    "export";
    "fi";
    "for";
    "function";
    "getopts";
    "hash";
    "help";
    "if";
    "jobs";
    "let";
    "local";
    "logout";
    "mapfile";
    "popd";
    "pushd";
    "pwd";
    "read";
    "readonly";
    "return";
    "select";
    "set";
    "shift";
    "source";
    "then";
    "times";
    "trap";
    "typeset";
    "ulimit";
    "umask";
    "unalias";
    "unset";
    "until";
    "wait";
    "while";
  ]

let make_id ~path ~index source =
  "n-"
  ^ String.sub (Sha256.hex (Printf.sprintf "%s:%d:%s" path index source)) 0 16

let residual ?interpreter ~path ~source ~reason () =
  let span = span_for_source ~path source in
  let interpreter =
    Option.value ~default:(interpreter_of_source source) interpreter
  in
  let capsule = Ir.opaque_file ~path ~interpreter ~source ~reason in
  let root =
    Ir.node
      ~id:(make_id ~path ~index:0 source)
      ~guarantee:(Ir.Residual { reason })
      ~source:span (Ir.Opaque_capsule capsule)
  in
  {
    root;
    diagnostics = [ { severity = Warning; message = reason; span = Some span } ];
  }

let lower_basic ~path source =
  match lex source with
  | Error reason -> residual ~path ~source ~reason ()
  | Ok items ->
      if
        List.exists
          (function
            | Word token -> token.dynamic | Pipe _ | Separator _ -> false)
          items
      then
        residual ~path ~source
          ~reason:"dynamic shell expansion is preserved in a residual capsule"
          ()
      else
        begin match parse_items items with
        | Error reason -> residual ~path ~source ~reason ()
        | Ok [] ->
            residual ~path ~source
              ~reason:"empty shell input has no executable effect" ()
        | Ok groups ->
            let unsupported = ref None in
            let next_id = ref 0 in
            let node_for_words words =
              let rec take_environment accumulator = function
                | token :: rest ->
                    begin match assignment token with
                    | Some pair -> take_environment (pair :: accumulator) rest
                    | None -> (List.rev accumulator, token :: rest)
                    end
                | [] -> (List.rev accumulator, [])
              in
              let environment, command_words = take_environment [] words in
              match command_words with
              | [] ->
                  unsupported :=
                    Some "standalone environment assignment needs shell state";
                  None
              | command :: _
                when List.mem
                       (String.lowercase_ascii command.text)
                       unsupported_commands ->
                  unsupported :=
                    Some
                      (Printf.sprintf
                         "shell builtin %S is outside the static literal subset"
                         command.text);
                  None
              | _ ->
                  incr next_id;
                  let argv = List.map (fun token -> token.text) command_words in
                  let first = List.hd words in
                  let last = List.hd (List.rev words) in
                  let source_span =
                    span_for_range ~path source ~start_byte:first.start_byte
                      ~end_byte:last.end_byte
                  in
                  let id =
                    make_id ~path ~index:!next_id (String.concat "\000" argv)
                  in
                  Some
                    (Ir.node ~id
                       ~guarantee:
                         (Ir.Formal
                            { basis = "posix-static-literal-command-v1" })
                       ~source:source_span
                       (Ir.Exec (Ir.exec ~environment argv)))
            in
            let stage_nodes =
              List.map
                (fun stages -> List.filter_map node_for_words stages)
                groups
            in
            begin match !unsupported with
            | Some reason -> residual ~path ~source ~reason ()
            | None ->
                let pipeline_nodes =
                  List.mapi
                    (fun index stages ->
                      match stages with
                      | [ node ] -> node
                      | nodes ->
                          let first_span =
                            Option.get (List.hd nodes).Ir.source
                          in
                          let last_span =
                            Option.get (List.hd (List.rev nodes)).Ir.source
                          in
                          Ir.node
                            ~id:(make_id ~path ~index:(10_000 + index) source)
                            ~guarantee:
                              (Ir.Formal { basis = "posix-static-pipeline-v1" })
                            ~source:(cover_spans first_span last_span)
                            (Ir.Pipeline nodes))
                    stage_nodes
                in
                let root =
                  match pipeline_nodes with
                  | [ node ] -> node
                  | nodes ->
                      let first_span = Option.get (List.hd nodes).Ir.source in
                      let last_span =
                        Option.get (List.hd (List.rev nodes)).Ir.source
                      in
                      Ir.node
                        ~id:(make_id ~path ~index:20_000 source)
                        ~guarantee:
                          (Ir.Formal { basis = "posix-static-sequence-v1" })
                        ~source:(cover_spans first_span last_span)
                        (Ir.Sequence nodes)
                in
                { root; diagnostics = [] }
            end
        end

let starts_at value ~offset needle =
  offset >= 0
  && offset + String.length needle <= String.length value
  && String.sub value offset (String.length needle) = needle

let find_top_level source ~from needle =
  let state = ref `Normal in
  let escaped = ref false in
  let found = ref None in
  let index = ref from in
  while
    !index + String.length needle <= String.length source && !found = None
  do
    let character = source.[!index] in
    begin match !state with
    | `Single -> if character = '\'' then state := `Normal
    | `Double ->
        if !escaped then escaped := false
        else if character = '\\' then escaped := true
        else if character = '"' then state := `Normal
    | `Normal ->
        if character = '\'' then state := `Single
        else if character = '"' then state := `Double
        else if character = '\\' then incr index
        else if starts_at source ~offset:!index needle then found := Some !index
    end;
    incr index
  done;
  !found

let trim_bounds source start_byte end_byte =
  let start_byte = ref start_byte in
  let end_byte = ref end_byte in
  let whitespace = function ' ' | '\t' | '\r' | '\n' -> true | _ -> false in
  while !start_byte < !end_byte && whitespace source.[!start_byte] do
    incr start_byte
  done;
  while !end_byte > !start_byte && whitespace source.[!end_byte - 1] do
    decr end_byte
  done;
  (!start_byte, !end_byte)

let rec relocate_node ~path ~source ~offset (node : Ir.node) =
  let relocate = relocate_node ~path ~source ~offset in
  let operation =
    match node.operation with
    | Ir.Exec command -> Ir.Exec command
    | Ir.Pipeline nodes -> Ir.Pipeline (List.map relocate nodes)
    | Ir.Sequence nodes -> Ir.Sequence (List.map relocate nodes)
    | Ir.Parallel nodes -> Ir.Parallel (List.map relocate nodes)
    | Ir.Condition { predicate; if_true; if_false } ->
        Ir.Condition
          {
            predicate = relocate predicate;
            if_true = relocate if_true;
            if_false = Option.map relocate if_false;
          }
    | Ir.Match { value; cases; default } ->
        Ir.Match
          {
            value;
            cases =
              List.map (fun (pattern, body) -> (pattern, relocate body)) cases;
            default = Option.map relocate default;
          }
    | Ir.For_each { variable; items; body } ->
        Ir.For_each { variable; items; body = relocate body }
    | Ir.Try_finally { body; finalizer } ->
        Ir.Try_finally { body = relocate body; finalizer = relocate finalizer }
    | Ir.Task_call call -> Ir.Task_call call
    | Ir.File_read value -> Ir.File_read value
    | Ir.File_write value -> Ir.File_write value
    | Ir.File_remove value -> Ir.File_remove value
    | Ir.Network_request value -> Ir.Network_request value
    | Ir.Opaque_capsule value -> Ir.Opaque_capsule value
  in
  let source_span =
    Option.map
      (fun span ->
        span_for_range ~path source
          ~start_byte:(offset + span.Ir.start_byte)
          ~end_byte:(offset + span.end_byte))
      node.source
  in
  {
    node with
    id = make_id ~path ~index:(30_000 + offset) node.id;
    operation;
    source = source_span;
  }

let has_residual node =
  Ir.fold_nodes
    (fun found child ->
      found
      || match child.Ir.guarantee with Ir.Residual _ -> true | _ -> false)
    false node

let lower_fragment ~path ~source start_byte end_byte =
  let start_byte, end_byte = trim_bounds source start_byte end_byte in
  if start_byte = end_byte then Error "control-flow command is empty"
  else
    let fragment = String.sub source start_byte (end_byte - start_byte) in
    let lowered = lower_basic ~path fragment in
    if lowered.diagnostics <> [] || has_residual lowered.root then
      Error "control-flow command is outside the static literal subset"
    else Ok (relocate_node ~path ~source ~offset:start_byte lowered.root)

let control_result ~path ~source ~start_byte ~end_byte ~basis operation =
  let span = span_for_range ~path source ~start_byte ~end_byte in
  {
    root =
      Ir.node
        ~id:(make_id ~path ~index:(40_000 + start_byte) source)
        ~guarantee:(Ir.Formal { basis })
        ~source:span operation;
    diagnostics = [];
  }

let parse_and ~path source start_byte end_byte =
  match find_top_level source ~from:start_byte "&&" with
  | Some separator when separator < end_byte ->
      begin match
        ( lower_fragment ~path ~source start_byte separator,
          lower_fragment ~path ~source (separator + 2) end_byte )
      with
      | Ok predicate, Ok if_true ->
          Some
            (control_result ~path ~source ~start_byte ~end_byte
               ~basis:"posix-static-and-condition-v1"
               (Ir.Condition { predicate; if_true; if_false = None }))
      | Error reason, _ | _, Error reason ->
          Some (residual ~path ~source ~reason ())
      end
  | _ -> None

let parse_if ~path source start_byte end_byte =
  if
    (not (starts_at source ~offset:start_byte "if "))
    || end_byte - start_byte < 7
    || not (starts_at source ~offset:(end_byte - 4) "; fi")
  then None
  else
    match find_top_level source ~from:(start_byte + 3) "; then " with
    | None -> None
    | Some then_separator ->
        let branch_start = then_separator + 7 in
        let final_separator = end_byte - 4 in
        let else_separator =
          match find_top_level source ~from:branch_start "; else " with
          | Some value when value < final_separator -> Some value
          | _ -> None
        in
        let true_end = Option.value ~default:final_separator else_separator in
        let predicate =
          lower_fragment ~path ~source (start_byte + 3) then_separator
        in
        let if_true = lower_fragment ~path ~source branch_start true_end in
        let if_false =
          Option.map
            (fun separator ->
              lower_fragment ~path ~source (separator + 7) final_separator)
            else_separator
        in
        begin match (predicate, if_true, if_false) with
        | Ok predicate, Ok if_true, None ->
            let if_false =
              Ir.node
                ~id:(make_id ~path ~index:(45_000 + start_byte) source)
                ~guarantee:
                  (Ir.Formal { basis = "posix-if-no-match-success-v1" })
                ~source:
                  (span_for_range ~path source ~start_byte:final_separator
                     ~end_byte:final_separator)
                (Ir.Sequence [])
            in
            Some
              (control_result ~path ~source ~start_byte ~end_byte
                 ~basis:"posix-static-condition-v1"
                 (Ir.Condition { predicate; if_true; if_false = Some if_false }))
        | Ok predicate, Ok if_true, Some (Ok if_false) ->
            Some
              (control_result ~path ~source ~start_byte ~end_byte
                 ~basis:"posix-static-condition-v1"
                 (Ir.Condition { predicate; if_true; if_false = Some if_false }))
        | Error reason, _, _ | _, Error reason, _ | _, _, Some (Error reason) ->
            Some (residual ~path ~source ~reason ())
        end

let replace_all value ~pattern ~replacement =
  let buffer = Buffer.create (String.length value) in
  let rec loop offset =
    if offset >= String.length value then Buffer.contents buffer
    else if starts_at value ~offset pattern then begin
      Buffer.add_string buffer replacement;
      loop (offset + String.length pattern)
    end
    else begin
      Buffer.add_char buffer value.[offset];
      loop (offset + 1)
    end
  in
  loop 0

let contains value needle =
  let rec loop offset =
    if offset + String.length needle > String.length value then false
    else if starts_at value ~offset needle then true
    else loop (offset + 1)
  in
  needle = "" || loop 0

let rec map_binding_argv markers template (node : Ir.node) =
  let map_value value =
    match List.find_opt (fun marker -> value = marker) markers with
    | Some _ -> Ok template
    | None when List.exists (contains value) markers ->
        Error "foreach variable must occupy a complete argument"
    | None -> Ok value
  in
  let rec map_values accumulator = function
    | [] -> Ok (List.rev accumulator)
    | value :: rest ->
        begin match map_value value with
        | Error _ as error -> error
        | Ok value -> map_values (value :: accumulator) rest
        end
  in
  let map_nodes nodes =
    let rec loop accumulator = function
      | [] -> Ok (List.rev accumulator)
      | value :: rest ->
          begin match map_binding_argv markers template value with
          | Error _ as error -> error
          | Ok value -> loop (value :: accumulator) rest
          end
    in
    loop [] nodes
  in
  match node.operation with
  | Ir.Exec command ->
      begin match map_values [] command.argv with
      | Error _ as error -> error
      | Ok argv -> Ok { node with operation = Ir.Exec { command with argv } }
      end
  | Ir.Pipeline nodes ->
      Result.map
        (fun nodes -> { node with operation = Ir.Pipeline nodes })
        (map_nodes nodes)
  | Ir.Sequence nodes ->
      Result.map
        (fun nodes -> { node with operation = Ir.Sequence nodes })
        (map_nodes nodes)
  | _ -> Error "foreach body contains unsupported control flow"

let parse_for ~path source start_byte end_byte =
  if
    (not (starts_at source ~offset:start_byte "for "))
    || end_byte - start_byte < 12
    || not (starts_at source ~offset:(end_byte - 6) "; done")
  then None
  else
    match find_top_level source ~from:(start_byte + 4) " in " with
    | None -> None
    | Some in_separator ->
        begin match find_top_level source ~from:(in_separator + 4) "; do " with
        | None -> None
        | Some do_separator ->
            let variable =
              String.sub source (start_byte + 4) (in_separator - start_byte - 4)
              |> String.trim
            in
            let items_source =
              String.sub source (in_separator + 4)
                (do_separator - in_separator - 4)
            in
            let body_start = do_separator + 5 in
            let body_end = end_byte - 6 in
            let items =
              match lex items_source with
              | Ok values ->
                  let rec collect accumulator = function
                    | [] when accumulator <> [] -> Ok (List.rev accumulator)
                    | Word token :: rest when not token.dynamic ->
                        collect (token.text :: accumulator) rest
                    | _ ->
                        Error "foreach items must be a non-empty literal list"
                  in
                  collect [] values
              | Error reason -> Error reason
            in
            if not (valid_environment_name variable) then
              Some
                (residual ~path ~source
                   ~reason:"foreach variable name is invalid" ())
            else
              let short_pattern = "$" ^ variable in
              let braced_pattern = "${" ^ variable ^ "}" in
              let marker length = "Z" ^ String.make (length - 1) '_' in
              let short_marker = marker (String.length short_pattern) in
              let braced_marker = marker (String.length braced_pattern) in
              let body_source =
                String.sub source body_start (body_end - body_start)
              in
              let rewritten_body =
                body_source
                |> replace_all ~pattern:braced_pattern
                     ~replacement:braced_marker
                |> replace_all ~pattern:short_pattern ~replacement:short_marker
              in
              begin match items with
              | Error reason -> Some (residual ~path ~source ~reason ())
              | Ok items ->
                  let lowered = lower_basic ~path rewritten_body in
                  if lowered.diagnostics <> [] || has_residual lowered.root then
                    Some
                      (residual ~path ~source
                         ~reason:
                           "foreach body is outside the static literal subset"
                         ())
                  else
                    begin match
                      map_binding_argv
                        [ short_marker; braced_marker ]
                        ("${" ^ variable ^ "}")
                        lowered.root
                    with
                    | Error reason -> Some (residual ~path ~source ~reason ())
                    | Ok body ->
                        let body =
                          relocate_node ~path ~source ~offset:body_start body
                        in
                        Some
                          (control_result ~path ~source ~start_byte ~end_byte
                             ~basis:"posix-static-foreach-v1"
                             (Ir.For_each { variable; items; body }))
                    end
              end
        end

let lower ~path source =
  let start_byte, end_byte = trim_bounds source 0 (String.length source) in
  match parse_if ~path source start_byte end_byte with
  | Some result -> result
  | None ->
      begin match parse_for ~path source start_byte end_byte with
      | Some result -> result
      | None ->
          begin match parse_and ~path source start_byte end_byte with
          | Some result -> result
          | None -> lower_basic ~path source
          end
      end
