type severity = Warning | Diagnostic_error

type diagnostic = {
  severity : severity;
  message : string;
  span : Ir.source_span option;
}

type result = { root : Ir.node; diagnostics : diagnostic list }
type token = { text : string; dynamic : bool; start_byte : int; end_byte : int }
type item = Word of token | Pipe of int | Separator of int

let ( let* ) result continuation =
  match result with Ok value -> continuation value | Error _ as error -> error

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
  let add_literal_dollar () =
    start_word !index;
    Buffer.add_string buffer "$$"
  in
  while !index < length && !failure = None do
    let character = source.[!index] in
    begin match !state with
    | `Single ->
        if character = '\'' then state := `Normal
        else if character = '$' then add_literal_dollar ()
        else add character
    | `Double ->
        if character = '"' then state := `Normal
        else if character = '\\' then
          if !index + 1 >= length then
            failure := Some "trailing escape in double-quoted string"
          else begin
            let escaped = source.[!index + 1] in
            if List.mem escaped [ '$'; '`'; '"'; '\\'; '\n' ] then begin
              incr index;
              if escaped = '$' then add_literal_dollar ()
              else if escaped <> '\n' then add escaped
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
              if source.[!index] = '$' then Buffer.add_string buffer "$$"
              else if source.[!index] <> '\n' then
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
        | '['
          when (not !started)
               && !index + 1 < length
               && List.mem source.[!index + 1] [ ' '; '\t'; '\r'; '\n' ] ->
            add character
        | ']'
          when (not !started)
               && (!index + 1 = length
                  || List.mem
                       source.[!index + 1]
                       [ ' '; '\t'; '\r'; '\n'; ';'; '|'; '&' ]) ->
            add character
        | '{'
          when (not !started)
               && !index + 1 < length
               && source.[!index + 1] = '}'
               && (!index + 2 = length
                  || List.mem
                       source.[!index + 2]
                       [ ' '; '\t'; '\r'; '\n'; ';'; '|'; '&' ]) ->
            add character;
            incr index;
            add source.[!index]
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
      let reason =
        match lowered.root.guarantee with
        | Ir.Residual evidence -> evidence.reason
        | _ -> "control-flow command is outside the static literal subset"
      in
      Error reason
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

let line_ranges source =
  let length = String.length source in
  let rec loop start index accumulator =
    if index = length then List.rev ((start, index) :: accumulator)
    else if source.[index] = '\n' then
      loop (index + 1) (index + 1) ((start, index) :: accumulator)
    else loop start (index + 1) accumulator
  in
  if length = 0 then [] else loop 0 0 []

let blank_range bytes start_byte end_byte =
  for index = start_byte to end_byte - 1 do
    if Bytes.get bytes index <> '\n' then Bytes.set bytes index ' '
  done

let parameter_name value =
  valid_environment_name value
  || value <> ""
     && String.for_all (function '0' .. '9' -> true | _ -> false) value

let static_parameter_default value =
  not
    (String.exists
       (function '$' | '`' | '{' | '}' | '\n' | '\r' -> true | _ -> false)
       value)

let parameter_template bindings expression =
  let parameter =
    match String.index_opt expression ':' with
    | Some separator
      when separator + 1 < String.length expression
           && expression.[separator + 1] = '-' ->
        Ok
          ( String.sub expression 0 separator,
            Some
              (String.sub expression (separator + 2)
                 (String.length expression - separator - 2)) )
    | Some _ -> Error "parameter operator is outside the strict static subset"
    | None -> Ok (expression, None)
  in
  let* name, default = parameter in
  if not (parameter_name name) then Error "invalid parameter expansion"
  else
    match default with
    | Some fallback when not (static_parameter_default fallback) ->
        Error "dynamic parameter default is outside the strict static subset"
    | Some fallback -> Ok (Printf.sprintf "${%s:-%s}" name fallback)
    | None ->
        begin match List.assoc_opt name bindings with
        | Some value -> Ok value
        | None -> Ok ("${" ^ name ^ "}")
        end

let expand_assignment_word bindings source =
  let length = String.length source in
  let output = Buffer.create length in
  let state = ref `Normal in
  let failure = ref None in
  let index = ref 0 in
  let add_literal_dollar () = Buffer.add_string output "$$" in
  let add_parameter start finish expression =
    match parameter_template bindings expression with
    | Error message -> failure := Some message
    | Ok value ->
        Buffer.add_string output value;
        index := finish;
        ignore start
  in
  let parse_parameter () =
    if !index + 1 >= length then failure := Some "trailing parameter marker"
    else
      match source.[!index + 1] with
      | '(' ->
          failure := Some "command substitution is outside the static subset"
      | '{' ->
          begin match String.index_from_opt source (!index + 2) '}' with
          | None -> failure := Some "unterminated parameter expansion"
          | Some close ->
              let expression =
                String.sub source (!index + 2) (close - !index - 2)
              in
              add_parameter !index close expression
          end
      | '0' .. '9' as digit ->
          add_parameter !index (!index + 1) (String.make 1 digit)
      | character
        when match character with
             | 'A' .. 'Z' | 'a' .. 'z' | '_' -> true
             | _ -> false ->
          let rec finish cursor =
            if cursor < length then
              match source.[cursor] with
              | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> finish (cursor + 1)
              | _ -> cursor
            else cursor
          in
          let finish = finish (!index + 1) in
          let name = String.sub source (!index + 1) (finish - !index - 1) in
          add_parameter !index (finish - 1) name
      | _ ->
          failure := Some "special shell parameter is outside the static subset"
  in
  while !index < length && !failure = None do
    let character = source.[!index] in
    begin match !state with
    | `Single ->
        if character = '\'' then state := `Normal
        else if character = '$' then add_literal_dollar ()
        else Buffer.add_char output character
    | `Double ->
        begin match character with
        | '"' -> state := `Normal
        | '$' -> parse_parameter ()
        | '`' ->
            failure := Some "command substitution is outside the static subset"
        | '\\' when !index + 1 < length ->
            let escaped = source.[!index + 1] in
            if List.mem escaped [ '$'; '`'; '"'; '\\'; '\n' ] then begin
              incr index;
              if escaped = '$' then add_literal_dollar ()
              else if escaped <> '\n' then Buffer.add_char output escaped
            end
            else Buffer.add_char output character
        | _ -> Buffer.add_char output character
        end
    | `Normal ->
        begin match character with
        | '\'' -> state := `Single
        | '"' -> state := `Double
        | '$' -> parse_parameter ()
        | '`' ->
            failure := Some "command substitution is outside the static subset"
        | '\\' when !index + 1 < length ->
            incr index;
            if source.[!index] = '$' then add_literal_dollar ()
            else if source.[!index] <> '\n' then
              Buffer.add_char output source.[!index]
        | ' ' | '\t' | '\r' | '\n' | ';' | '|' | '&' | '<' | '>' | '*' | '?'
        | '[' | ']' ->
            failure := Some "assignment word requires dynamic shell semantics"
        | _ -> Buffer.add_char output character
        end
    end;
    incr index
  done;
  match (!failure, !state) with
  | Some message, _ -> Error message
  | None, (`Single | `Double) -> Error "unterminated assignment quote"
  | None, `Normal -> Ok (Buffer.contents output)

let standalone_assignment line =
  match lex line with
  | Ok [ Word token ] when Option.is_some (assignment token) ->
      begin match String.index_opt line '=' with
      | None -> None
      | Some separator ->
          let name = String.sub line 0 separator |> String.trim in
          if not (valid_environment_name name) then None
          else
            let prefix = String.sub line 0 separator in
            if String.trim prefix <> prefix then None
            else
              Some
                ( name,
                  String.sub line (separator + 1)
                    (String.length line - separator - 1) )
      end
  | Ok _ | Error _ -> None

let strip_shell_comment line =
  let length = String.length line in
  let rec loop index state escaped =
    if index >= length then line
    else
      let character = line.[index] in
      match state with
      | `Single ->
          if character = '\'' then loop (index + 1) `Normal false
          else loop (index + 1) `Single false
      | `Double ->
          if escaped then loop (index + 1) `Double false
          else if character = '\\' then loop (index + 1) `Double true
          else if character = '"' then loop (index + 1) `Normal false
          else loop (index + 1) `Double false
      | `Normal ->
          if escaped then loop (index + 1) `Normal false
          else if character = '\\' then loop (index + 1) `Normal true
          else if character = '\'' then loop (index + 1) `Single false
          else if character = '"' then loop (index + 1) `Double false
          else if
            character = '#'
            && (index = 0 || List.mem line.[index - 1] [ ' '; '\t'; '\r' ])
          then String.sub line 0 index
          else loop (index + 1) `Normal false
  in
  loop 0 `Normal false |> String.trim

type strict_options = { errexit : bool; nounset : bool; pipefail : bool }

let no_strict_options = { errexit = false; nounset = false; pipefail = false }

let merge_strict_options left right =
  {
    errexit = left.errexit || right.errexit;
    nounset = left.nounset || right.nounset;
    pipefail = left.pipefail || right.pipefail;
  }

let parse_strict_option_words words =
  let options = ref no_strict_options in
  let expects_named_option = ref false in
  let failure = ref None in
  let enable_named value =
    match value with
    | "errexit" ->
        options := { !options with errexit = true };
        Ok ()
    | "nounset" ->
        options := { !options with nounset = true };
        Ok ()
    | "pipefail" ->
        options := { !options with pipefail = true };
        Ok ()
    | _ -> Error ("shell option is outside the strict static subset: " ^ value)
  in
  List.iter
    (fun word ->
      if !failure = None then
        if !expects_named_option then begin
          expects_named_option := false;
          match enable_named word with
          | Ok () -> ()
          | Error message -> failure := Some message
        end
        else if String.length word > 1 && word.[0] = '-' then
          let flags =
            String.sub word 1 (String.length word - 1)
            |> String.to_seq |> List.of_seq
          in
          List.iter
            (fun flag ->
              if !failure = None then
                match flag with
                | 'e' -> options := { !options with errexit = true }
                | 'u' -> options := { !options with nounset = true }
                | 'o' -> expects_named_option := true
                | _ ->
                    failure :=
                      Some
                        (Printf.sprintf
                           "shell option -%c is outside the strict static \
                            subset"
                           flag))
            flags
        else
          failure :=
            Some ("shell option argument is outside the static subset: " ^ word))
    words;
  match !failure with
  | Some _ as error -> Error (Option.get error)
  | None when !expects_named_option ->
      Error "set -o requires a named shell option"
  | None -> Ok !options

let strict_set line =
  match split_words line with
  | "set" :: (_ :: _ as words) -> parse_strict_option_words words
  | _ -> Error "not a shell option declaration"

let shebang_strict_options source =
  let first_line =
    match String.split_on_char '\n' source with line :: _ -> line | [] -> ""
  in
  if not (String.starts_with ~prefix:"#!" first_line) then
    Ok (false, no_strict_options)
  else
    let words =
      String.sub first_line 2 (String.length first_line - 2)
      |> String.trim |> split_words
    in
    let option_words =
      match words with
      | [] -> []
      | executable :: rest when normalize_interpreter executable = "env" ->
          let rec after_program = function
            | [] -> []
            | "-S" :: remaining -> after_program remaining
            | option :: remaining
              when String.length option > 0 && option.[0] = '-' ->
                after_program remaining
            | _program :: remaining -> remaining
          in
          after_program rest
      | _executable :: rest -> rest
    in
    match option_words with
    | [] -> Ok (false, no_strict_options)
    | words ->
        Result.map
          (fun options -> (true, options))
          (parse_strict_option_words words)

let marker_for source used index length =
  let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ" in
  let rec candidate nonce =
    if nonce >= 26 * 26 then None
    else
      let value =
        String.init length (fun position ->
            if position = 0 then 'Z'
            else
              alphabet.[(nonce + (position * 7) + (index * 11))
                        mod String.length alphabet])
      in
      if contains source value || List.mem_assoc value used then
        candidate (nonce + 1)
      else Some value
  in
  candidate 0

let rewrite_command_parameters ~bindings source =
  let bytes = Bytes.of_string source in
  let length = String.length source in
  let mappings = ref [] in
  let state = ref `Normal in
  let word_started = ref false in
  let failure = ref None in
  let index = ref 0 in
  let replace_parameter start finish expression =
    match parameter_template bindings expression with
    | Error message -> failure := Some message
    | Ok template ->
        let marker_length = finish - start in
        begin match
          marker_for source !mappings (List.length !mappings) marker_length
        with
        | None -> failure := Some "too many parameter references to map safely"
        | Some marker ->
            Bytes.blit_string marker 0 bytes start marker_length;
            mappings := (marker, template) :: !mappings;
            index := finish - 1
        end
  in
  let parse_parameter quoted =
    if not quoted then
      failure :=
        Some "unquoted parameter expansion requires field splitting semantics"
    else if !index + 1 >= length then
      failure := Some "trailing parameter marker"
    else
      match source.[!index + 1] with
      | '(' ->
          failure := Some "command substitution is outside the static subset"
      | '{' ->
          begin match String.index_from_opt source (!index + 2) '}' with
          | None -> failure := Some "unterminated parameter expansion"
          | Some close ->
              let expression =
                String.sub source (!index + 2) (close - !index - 2)
              in
              replace_parameter !index (close + 1) expression
          end
      | '0' .. '9' as digit ->
          replace_parameter !index (!index + 2) (String.make 1 digit)
      | character
        when match character with
             | 'A' .. 'Z' | 'a' .. 'z' | '_' -> true
             | _ -> false ->
          let rec finish cursor =
            if cursor < length then
              match source.[cursor] with
              | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> finish (cursor + 1)
              | _ -> cursor
            else cursor
          in
          let finish = finish (!index + 1) in
          let name = String.sub source (!index + 1) (finish - !index - 1) in
          replace_parameter !index finish name
      | _ ->
          failure := Some "special shell parameter is outside the static subset"
  in
  while !index < length && !failure = None do
    let character = source.[!index] in
    begin match !state with
    | `Single -> if character = '\'' then state := `Normal
    | `Double ->
        begin match character with
        | '"' -> state := `Normal
        | '\\' -> incr index
        | '$' -> parse_parameter true
        | '`' ->
            failure := Some "command substitution is outside the static subset"
        | _ -> ()
        end
    | `Normal ->
        begin match character with
        | '\'' ->
            state := `Single;
            word_started := true
        | '"' ->
            state := `Double;
            word_started := true
        | '\\' ->
            incr index;
            word_started := true
        | '#' when not !word_started ->
            while !index < length && source.[!index] <> '\n' do
              incr index
            done;
            word_started := false
        | '$' -> parse_parameter false
        | '`' ->
            failure := Some "command substitution is outside the static subset"
        | ' ' | '\t' | '\r' | '\n' | ';' | '|' | '&' -> word_started := false
        | _ -> word_started := true
        end
    end;
    incr index
  done;
  match (!failure, !state) with
  | Some message, _ -> Error message
  | None, (`Single | `Double) -> Error "unterminated command quote"
  | None, `Normal -> Ok (Bytes.to_string bytes, List.rev !mappings)

let apply_template_mappings mappings value =
  List.fold_left
    (fun value (marker, template) ->
      replace_all value ~pattern:marker ~replacement:template)
    value mappings

let rec map_template_node mappings (node : Ir.node) =
  let map = map_template_node mappings in
  let map_value = apply_template_mappings mappings in
  let operation =
    match node.operation with
    | Ir.Exec command ->
        Ir.Exec
          {
            argv = List.map map_value command.argv;
            environment =
              List.map
                (fun (name, value) -> (name, map_value value))
                command.environment;
            working_directory = Option.map map_value command.working_directory;
          }
    | Ir.Pipeline nodes -> Ir.Pipeline (List.map map nodes)
    | Ir.Sequence nodes -> Ir.Sequence (List.map map nodes)
    | Ir.Parallel nodes -> Ir.Parallel (List.map map nodes)
    | Ir.Condition { predicate; if_true; if_false } ->
        Ir.Condition
          {
            predicate = map predicate;
            if_true = map if_true;
            if_false = Option.map map if_false;
          }
    | Ir.Match { value; cases; default } ->
        Ir.Match
          {
            value = map_value value;
            cases = List.map (fun (pattern, body) -> (pattern, map body)) cases;
            default = Option.map map default;
          }
    | Ir.For_each { variable; items; body } ->
        Ir.For_each
          { variable; items = List.map map_value items; body = map body }
    | Ir.Try_finally { body; finalizer } ->
        Ir.Try_finally { body = map body; finalizer = map finalizer }
    | Ir.Task_call call ->
        Ir.Task_call
          {
            call with
            arguments =
              List.map
                (fun (name, value) -> (name, map_value value))
                call.arguments;
          }
    | Ir.File_read value -> Ir.File_read (map_value value)
    | Ir.File_write write ->
        Ir.File_write
          {
            write with
            path = map_value write.path;
            contents = map_value write.contents;
          }
    | Ir.File_remove value -> Ir.File_remove (map_value value)
    | Ir.Network_request request ->
        Ir.Network_request
          { method_ = map_value request.method_; uri = map_value request.uri }
    | Ir.Opaque_capsule capsule -> Ir.Opaque_capsule capsule
  in
  { node with operation }

let fail_fast_sequence ~path ~source nodes =
  let next_id = ref 0 in
  let rec chain = function
    | [] ->
        Ir.node
          ~id:(make_id ~path ~index:69_999 source)
          ~guarantee:(Ir.Formal { basis = "posix-strict-empty-v1" })
          (Ir.Sequence [])
    | [ node ] -> node
    | predicate :: rest ->
        incr next_id;
        let node_index = 70_000 + !next_id in
        let if_true = chain rest in
        let source_span =
          match (predicate.Ir.source, if_true.Ir.source) with
          | Some first, Some last -> Some (cover_spans first last)
          | Some span, None | None, Some span -> Some span
          | None, None -> None
        in
        Ir.node ?source:source_span
          ~id:(make_id ~path ~index:node_index source)
          ~guarantee:(Ir.Formal { basis = "posix-set-e-fail-fast-v1" })
          (Ir.Condition { predicate; if_true; if_false = None })
  in
  chain nodes

let next_line_start source end_byte =
  if end_byte < String.length source && source.[end_byte] = '\n' then
    end_byte + 1
  else end_byte

let line_text source (start_byte, end_byte) =
  String.sub source start_byte (end_byte - start_byte)

let line_is_if value =
  let value = String.trim value in
  value = "if" || String.starts_with ~prefix:"if " value

let line_is_keyword keyword value = String.trim value = keyword

let line_continues value =
  let value = String.trim value in
  List.exists
    (fun suffix -> String.ends_with ~suffix value)
    [ "\\"; "&&"; "||"; "|" ]

type static_heredoc = {
  operator_start : int;
  marker_end : int;
  delimiter : string;
  quoted : bool;
}

let valid_heredoc_delimiter delimiter =
  delimiter <> ""
  && String.for_all
       (function
         | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> true | _ -> false)
       delimiter

let static_heredoc_delimiter value =
  let rec search index =
    if index + 2 > String.length value then None
    else if
      starts_at value ~offset:index "<<"
      && (index = 0 || value.[index - 1] <> '<')
    then
      let rec skip_space cursor =
        if cursor < String.length value then
          match value.[cursor] with
          | ' ' | '\t' -> skip_space (cursor + 1)
          | _ -> cursor
        else cursor
      in
      let marker_start = skip_space (index + 2) in
      if marker_start >= String.length value then None
      else
        match value.[marker_start] with
        | '-' | '<' -> search (index + 2)
        | ('\'' | '"') as quote ->
            begin match
              String.index_from_opt value (marker_start + 1) quote
            with
            | Some close when close > marker_start + 1 ->
                let delimiter =
                  String.sub value (marker_start + 1) (close - marker_start - 1)
                in
                if valid_heredoc_delimiter delimiter then
                  Some
                    {
                      operator_start = index;
                      marker_end = close + 1;
                      delimiter;
                      quoted = true;
                    }
                else None
            | Some _ | None -> None
            end
        | _ ->
            let rec finish cursor =
              if cursor < String.length value then
                match value.[cursor] with
                | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' ->
                    finish (cursor + 1)
                | _ -> cursor
              else cursor
            in
            let marker_end = finish marker_start in
            let delimiter =
              String.sub value marker_start (marker_end - marker_start)
            in
            if valid_heredoc_delimiter delimiter then
              Some
                {
                  operator_start = index;
                  marker_end;
                  delimiter;
                  quoted = false;
                }
            else search (index + 2)
    else search (index + 1)
  in
  search 0

let line_without_carriage_return value =
  if String.ends_with ~suffix:"\r" value then
    String.sub value 0 (String.length value - 1)
  else value

let join_working_directory parent child =
  if not (Filename.is_relative child) then child
  else if parent = "." || parent = "" then child
  else if child = "." || child = "" then parent
  else parent ^ "/" ^ child

let prefix_effect_path directory path =
  if directory = "." || not (Filename.is_relative path) then path
  else join_working_directory directory path

let rec apply_working_directory directory (node : Ir.node) =
  let apply = apply_working_directory directory in
  let operation =
    match node.operation with
    | Ir.Exec command ->
        let working_directory =
          match command.working_directory with
          | None -> Some directory
          | Some child -> Some (join_working_directory directory child)
        in
        Ir.Exec { command with working_directory }
    | Ir.Pipeline nodes -> Ir.Pipeline (List.map apply nodes)
    | Ir.Sequence nodes -> Ir.Sequence (List.map apply nodes)
    | Ir.Parallel nodes -> Ir.Parallel (List.map apply nodes)
    | Ir.Condition { predicate; if_true; if_false } ->
        Ir.Condition
          {
            predicate = apply predicate;
            if_true = apply if_true;
            if_false = Option.map apply if_false;
          }
    | Ir.Match { value; cases; default } ->
        Ir.Match
          {
            value;
            cases =
              List.map (fun (pattern, body) -> (pattern, apply body)) cases;
            default = Option.map apply default;
          }
    | Ir.For_each { variable; items; body } ->
        Ir.For_each { variable; items; body = apply body }
    | Ir.Try_finally { body; finalizer } ->
        Ir.Try_finally { body = apply body; finalizer = apply finalizer }
    | Ir.Task_call call -> Ir.Task_call call
    | Ir.File_read path -> Ir.File_read (prefix_effect_path directory path)
    | Ir.File_write write ->
        Ir.File_write
          { write with path = prefix_effect_path directory write.path }
    | Ir.File_remove path -> Ir.File_remove (prefix_effect_path directory path)
    | Ir.Network_request request -> Ir.Network_request request
    | Ir.Opaque_capsule capsule -> Ir.Opaque_capsule capsule
  in
  { node with operation }

let strict_statement_ranges source start_byte end_byte =
  let lines =
    line_ranges source
    |> List.filter (fun (start, finish) ->
        finish > start_byte && start < end_byte)
  in
  let rec take_if depth finish = function
    | [] -> Error "multiline if is missing fi"
    | ((line_start, line_end) as line) :: rest ->
        let text = line_text source line in
        if line_is_if text then take_if (depth + 1) line_end rest
        else if line_is_keyword "fi" text then
          if depth = 1 then Ok (line_end, rest)
          else take_if (depth - 1) line_end rest
        else take_if depth (max finish line_end) rest
  in
  let rec take_continuation finish previous = function
    | [] ->
        if line_continues previous then
          Error "continued command is missing its next line"
        else Ok (finish, [])
    | ((_, line_end) as line) :: rest ->
        if not (line_continues previous) then Ok (finish, line :: rest)
        else
          let text = line_text source line in
          take_continuation line_end text rest
  in
  let rec take_heredoc delimiter = function
    | [] -> Error ("literal heredoc is missing delimiter " ^ delimiter)
    | ((_, line_end) as line) :: rest ->
        let text = line_text source line |> line_without_carriage_return in
        if text = delimiter then Ok (line_end, rest)
        else take_heredoc delimiter rest
  in
  let rec collect accumulator = function
    | [] -> Ok (List.rev accumulator)
    | ((line_start, line_end) as line) :: rest ->
        let text = line_text source line in
        let trimmed = String.trim text in
        if trimmed = "" || String.starts_with ~prefix:"#" trimmed then
          collect accumulator rest
        else if contains trimmed "<<-" then
          Error "tab-stripping heredoc syntax is outside the static subset"
        else if Option.is_some (static_heredoc_delimiter trimmed) then
          let heredoc = Option.get (static_heredoc_delimiter trimmed) in
          begin match take_heredoc heredoc.delimiter rest with
          | Error _ as error -> error
          | Ok (finish, remaining) ->
              collect ((line_start, finish) :: accumulator) remaining
          end
        else if line_is_if text && not (contains trimmed "; fi") then
          begin match take_if 1 line_end rest with
          | Error _ as error -> error
          | Ok (finish, remaining) ->
              collect ((line_start, finish) :: accumulator) remaining
          end
        else if
          List.exists
            (fun keyword -> line_is_keyword keyword text)
            [ "then"; "else"; "elif"; "fi" ]
        then Error ("unexpected shell control keyword: " ^ trimmed)
        else
          begin match take_continuation line_end text rest with
          | Error _ as error -> error
          | Ok (finish, remaining) ->
              collect ((line_start, finish) :: accumulator) remaining
          end
  in
  collect [] lines

let first_non_space source start_byte end_byte =
  let rec loop index =
    if index >= end_byte then end_byte
    else
      match source.[index] with
      | ' ' | '\t' | '\r' | '\n' -> loop (index + 1)
      | _ -> index
  in
  loop start_byte

let literal_word source =
  match lex source with
  | Ok [ Word token ] when not token.dynamic -> Ok token.text
  | Ok _ -> Error "file path must be one static shell word"
  | Error message -> Error message

let lower_literal_heredoc ~path ~source start_byte end_byte =
  let opener_end =
    match String.index_from_opt source start_byte '\n' with
    | Some value when value < end_byte -> value
    | Some _ | None -> end_byte
  in
  let opener =
    String.sub source start_byte (opener_end - start_byte) |> String.trim
  in
  match static_heredoc_delimiter opener with
  | None -> Error "literal heredoc marker is invalid"
  | Some heredoc ->
      let suffix =
        String.sub opener heredoc.marker_end
          (String.length opener - heredoc.marker_end)
        |> String.trim
      in
      if suffix <> "" then
        Error "tokens after a literal heredoc marker are outside the subset"
      else
        begin match String.index_opt opener '>' with
        | None -> Error "literal heredoc requires an output redirection"
        | Some redirect
          when redirect + 1 < String.length opener
               && opener.[redirect + 1] = '>' ->
            Error "append heredoc redirection is outside the static subset"
        | Some redirect ->
            let command = String.sub opener 0 redirect |> String.trim in
            if command <> "cat" then
              Error "only a literal cat heredoc can lower to FileWrite"
            else
              let path_source =
                String.sub opener (redirect + 1)
                  (heredoc.operator_start - redirect - 1)
                |> String.trim
              in
              let* output_path = literal_word path_source in
              let content_start =
                if opener_end < end_byte then opener_end + 1 else opener_end
              in
              let delimiter_start =
                line_ranges source
                |> List.find_map (fun (line_start, line_end) ->
                    if line_start < content_start || line_end > end_byte then
                      None
                    else
                      let line =
                        line_text source (line_start, line_end)
                        |> line_without_carriage_return
                      in
                      if line = heredoc.delimiter then Some line_start else None)
              in
              begin match delimiter_start with
              | None ->
                  Error
                    ("literal heredoc is missing delimiter " ^ heredoc.delimiter)
              | Some delimiter_start ->
                  let contents =
                    String.sub source content_start
                      (delimiter_start - content_start)
                    |> replace_all ~pattern:"$" ~replacement:"$$"
                  in
                  Ok
                    (Ir.node
                       ~id:(make_id ~path ~index:(65_000 + start_byte) source)
                       ~guarantee:
                         (Ir.Formal
                            { basis = "posix-literal-heredoc-file-write-v1" })
                       ~source:
                         (span_for_range ~path source ~start_byte ~end_byte)
                       (Ir.File_write
                          { path = output_path; contents; append = false }))
              end
        end

let static_cd_directory source =
  match lex source with
  | Ok [ Word command; Word directory ]
    when command.text = "cd" && not directory.dynamic ->
      Ok directory.text
  | Ok _ -> Error "subshell must begin with one static cd"
  | Error message -> Error message

let lower_static_subshell_cwd ~path ~source start_byte end_byte =
  let start_byte, end_byte = trim_bounds source start_byte end_byte in
  if
    end_byte - start_byte < 5
    || source.[start_byte] <> '('
    || source.[end_byte - 1] <> ')'
  then Error "not a static cwd subshell"
  else
    let inner_start, inner_end =
      trim_bounds source (start_byte + 1) (end_byte - 1)
    in
    match find_top_level source ~from:inner_start "&&" with
    | None -> Error "cwd subshell requires &&"
    | Some separator when separator >= inner_end ->
        Error "cwd subshell requires a command after &&"
    | Some separator ->
        let cd_source =
          String.sub source inner_start (separator - inner_start) |> String.trim
        in
        let* directory = static_cd_directory cd_source in
        let* command = lower_fragment ~path ~source (separator + 2) inner_end in
        let command = apply_working_directory directory command in
        Ok
          {
            command with
            id = make_id ~path ~index:(67_000 + start_byte) source;
            guarantee = Ir.Formal { basis = "posix-static-subshell-cwd-v1" };
            source = Some (span_for_range ~path source ~start_byte ~end_byte);
          }

let rec lower_strict_range ~path ~source start_byte end_byte =
  let start_byte, end_byte = trim_bounds source start_byte end_byte in
  if start_byte = end_byte then Error "strict command is empty"
  else if
    Option.is_some
      (static_heredoc_delimiter (line_text source (start_byte, end_byte)))
  then lower_literal_heredoc ~path ~source start_byte end_byte
  else if source.[start_byte] = '(' && source.[end_byte - 1] = ')' then
    lower_static_subshell_cwd ~path ~source start_byte end_byte
  else if starts_at source ~offset:start_byte "if " then
    begin match parse_if ~path source start_byte end_byte with
    | Some result when result.diagnostics = [] && not (has_residual result.root)
      ->
        Ok result.root
    | Some _ | None ->
        lower_strict_multiline_if ~path ~source start_byte end_byte
    end
  else
    let within_range operator =
      match find_top_level source ~from:start_byte operator with
      | Some separator when separator < end_byte -> Some separator
      | Some _ | None -> None
    in
    match (within_range "&&", within_range "||") with
    | Some _, Some _ -> Error "mixed && and || chains require explicit grouping"
    | Some separator, None ->
        let* predicate =
          lower_strict_range ~path ~source start_byte separator
        in
        let* if_true =
          lower_strict_range ~path ~source (separator + 2) end_byte
        in
        let span = span_for_range ~path source ~start_byte ~end_byte in
        Ok
          (Ir.node
             ~id:(make_id ~path ~index:(80_000 + start_byte) source)
             ~guarantee:(Ir.Formal { basis = "posix-strict-and-condition-v1" })
             ~source:span
             (Ir.Condition { predicate; if_true; if_false = None }))
    | None, Some separator ->
        let* predicate =
          lower_strict_range ~path ~source start_byte separator
        in
        let* if_false =
          lower_strict_range ~path ~source (separator + 2) end_byte
        in
        let success =
          Ir.node
            ~id:(make_id ~path ~index:(85_000 + separator) source)
            ~guarantee:
              (Ir.Formal { basis = "posix-strict-or-short-circuit-v1" })
            ~source:
              (span_for_range ~path source ~start_byte:separator
                 ~end_byte:(separator + 2))
            (Ir.Sequence [])
        in
        let span = span_for_range ~path source ~start_byte ~end_byte in
        Ok
          (Ir.node
             ~id:(make_id ~path ~index:(86_000 + start_byte) source)
             ~guarantee:(Ir.Formal { basis = "posix-strict-or-condition-v1" })
             ~source:span
             (Ir.Condition
                { predicate; if_true = success; if_false = Some if_false }))
    | None, None -> lower_fragment ~path ~source start_byte end_byte

and lower_strict_sequence ~path ~source start_byte end_byte =
  let* ranges = strict_statement_ranges source start_byte end_byte in
  let rec lower accumulator = function
    | [] -> Ok (List.rev accumulator)
    | (start_byte, end_byte) :: rest ->
        let* node = lower_strict_range ~path ~source start_byte end_byte in
        lower (node :: accumulator) rest
  in
  let* nodes = lower [] ranges in
  Ok (fail_fast_sequence ~path ~source nodes)

and lower_strict_multiline_if ~path ~source start_byte end_byte =
  let lines =
    line_ranges source
    |> List.filter (fun (start, finish) ->
        finish > start_byte && start < end_byte)
  in
  match lines with
  | [] -> Error "multiline if is empty"
  | (first_start, first_end) :: rest ->
      let first_keyword = first_non_space source first_start first_end in
      if
        first_keyword + 2 > first_end
        || not (starts_at source ~offset:first_keyword "if")
      then Error "multiline if does not start with if"
      else
        let predicate_start =
          first_non_space source (first_keyword + 2) first_end
        in
        let then_line = ref None in
        let else_line = ref None in
        let fi_line = ref None in
        let depth = ref 1 in
        let failure = ref None in
        List.iter
          (fun ((line_start, line_end) as line) ->
            if !failure = None && !fi_line = None then
              let text = line_text source line in
              if line_is_if text then incr depth
              else if line_is_keyword "fi" text then
                if !depth = 1 then fi_line := Some (line_start, line_end)
                else decr depth
              else if !depth = 1 && line_is_keyword "then" text then
                if !then_line = None then
                  then_line := Some (line_start, line_end)
                else failure := Some "multiline if has multiple then keywords"
              else if !depth = 1 && line_is_keyword "else" text then
                if !then_line = None then
                  failure := Some "multiline if has else before then"
                else if !else_line = None then
                  else_line := Some (line_start, line_end)
                else failure := Some "multiline if has multiple else keywords"
              else if !depth = 1 && line_is_keyword "elif" text then
                failure := Some "elif is outside the strict static subset")
          rest;
        begin match (!failure, !then_line, !fi_line) with
        | Some message, _, _ -> Error message
        | None, None, _ -> Error "multiline if is missing then"
        | None, _, None -> Error "multiline if is missing fi"
        | None, Some (then_start, then_end), Some (fi_start, _) ->
            let* predicate =
              lower_strict_range ~path ~source predicate_start then_start
            in
            let true_end =
              Option.fold ~none:fi_start
                ~some:(fun (start, _) -> start)
                !else_line
            in
            let* if_true =
              lower_strict_sequence ~path ~source
                (next_line_start source then_end)
                true_end
            in
            let* if_false =
              match !else_line with
              | Some (_, else_end) ->
                  let* branch =
                    lower_strict_sequence ~path ~source
                      (next_line_start source else_end)
                      fi_start
                  in
                  Ok (Some branch)
              | None ->
                  Ok
                    (Some
                       (Ir.node
                          ~id:
                            (make_id ~path ~index:(85_000 + start_byte) source)
                          ~guarantee:
                            (Ir.Formal
                               { basis = "posix-if-no-match-success-v1" })
                          ~source:
                            (span_for_range ~path source ~start_byte:fi_start
                               ~end_byte:fi_start)
                          (Ir.Sequence [])))
            in
            Ok
              (Ir.node
                 ~id:(make_id ~path ~index:(86_000 + start_byte) source)
                 ~guarantee:
                   (Ir.Formal { basis = "posix-strict-multiline-if-v1" })
                 ~source:(span_for_range ~path source ~start_byte ~end_byte)
                 (Ir.Condition { predicate; if_true; if_false }))
        end

let normalize_directory value =
  String.map
    (fun character -> if character = '\\' then '/' else character)
    value

let script_directory_change ~path line =
  let script_directory = Filename.dirname path |> normalize_directory in
  let script_directory =
    if script_directory = "" then "." else script_directory
  in
  match String.trim line with
  | "cd \"$(dirname \"$0\")\"" | "cd -- \"$(dirname -- \"$0\")\"" ->
      Some script_directory
  | "cd \"$(dirname \"$0\")/..\"" | "cd -- \"$(dirname -- \"$0\")/..\"" ->
      Some (join_working_directory script_directory "..")
  | _ -> None

let protect_literal_heredoc_dollars source rewritten =
  let markers = ref [] in
  let marker_for original =
    match List.assoc_opt original !markers with
    | Some marker -> Some marker
    | None ->
        let rec choose code =
          if code >= 32 then None
          else
            let character = Char.chr code in
            if
              List.mem character [ '\t'; '\n'; '\r' ]
              || String.contains source character
              || List.exists (fun (_, used) -> used = character) !markers
            then choose (code + 1)
            else Some character
        in
        begin match choose 1 with
        | None -> None
        | Some marker ->
            markers := (original, marker) :: !markers;
            Some marker
        end
  in
  let active_delimiter = ref None in
  let failure = ref None in
  List.iter
    (fun ((start_byte, end_byte) as range) ->
      if !failure = None then
        let line = line_text source range |> line_without_carriage_return in
        match !active_delimiter with
        | None ->
            begin match static_heredoc_delimiter (String.trim line) with
            | None -> ()
            | Some heredoc -> active_delimiter := Some heredoc
            end
        | Some heredoc when line = heredoc.delimiter -> active_delimiter := None
        | Some heredoc ->
            for index = start_byte to end_byte - 1 do
              let character = source.[index] in
              if heredoc.quoted && List.mem character [ '$'; '`' ] then
                begin match marker_for character with
                | None ->
                    failure :=
                      Some "no collision-free marker for literal heredoc data"
                | Some marker -> Bytes.set rewritten index marker
                end
              else if
                (not heredoc.quoted) && List.mem character [ '$'; '`'; '\\' ]
              then
                failure :=
                  Some
                    "unquoted heredoc body requires shell expansion semantics"
            done)
    (line_ranges source);
  match (!failure, !active_delimiter) with
  | Some message, _ -> Error message
  | None, Some heredoc ->
      Error ("literal heredoc is missing delimiter " ^ heredoc.delimiter)
  | None, None ->
      Ok
        (List.rev_map
           (fun (original, marker) ->
             ( String.make 1 marker,
               if original = '$' then "$$" else String.make 1 original ))
           !markers)

let parameter_mentioned_before source end_byte name =
  let prefix = String.sub source 0 end_byte in
  contains prefix ("$" ^ name) || contains prefix ("${" ^ name)

let strict_control_flow_line line =
  let trimmed = String.trim line in
  line_is_if line
  || List.exists
       (fun keyword -> line_is_keyword keyword line)
       [ "then"; "else"; "elif"; "fi"; "do"; "done"; "esac" ]
  || List.exists
       (fun prefix -> String.starts_with ~prefix trimmed)
       [ "for "; "while "; "until "; "case "; "select "; "function "; "("; "{" ]
  || List.exists (contains trimmed) [ "&&"; "||"; "|"; "()" ]

let lower_strict_script ~path source =
  let rewritten = Bytes.of_string source in
  let bindings = ref [] in
  let in_header = ref true in
  let working_directory = ref None in
  let active_heredoc = ref None in
  let saw_control_flow = ref false in
  let saw_shell_options = ref false in
  let shell_options = ref no_strict_options in
  let failure = ref None in
  begin match shebang_strict_options source with
  | Ok (saw_options, options) ->
      saw_shell_options := saw_options;
      shell_options := options
  | Error message ->
      saw_shell_options := true;
      failure := Some message
  end;
  List.iter
    (fun (start_byte, end_byte) ->
      if !failure = None then
        let line = String.sub source start_byte (end_byte - start_byte) in
        let physical_line = line_without_carriage_return line in
        match !active_heredoc with
        | Some delimiter ->
            if physical_line = delimiter then active_heredoc := None
        | None -> (
            let semantic_line = strip_shell_comment line in
            let trimmed = String.trim semantic_line in
            let opened_heredoc = static_heredoc_delimiter trimmed in
            if !in_header then
              begin if trimmed = "" || String.starts_with ~prefix:"#" trimmed
              then ()
              else if String.starts_with ~prefix:"set " trimmed then
                begin match strict_set trimmed with
                | Ok options ->
                    saw_shell_options := true;
                    shell_options := merge_strict_options !shell_options options;
                    blank_range rewritten start_byte end_byte
                | Error message -> failure := Some message
                end
              else if Option.is_some (script_directory_change ~path trimmed)
              then
                begin match !working_directory with
                | Some _ ->
                    failure :=
                      Some
                        "multiple script-directory changes require shell state"
                | None ->
                    working_directory := script_directory_change ~path trimmed;
                    blank_range rewritten start_byte end_byte
                end
              else
                match standalone_assignment semantic_line with
                | Some (name, word) ->
                    begin match expand_assignment_word !bindings word with
                    | Error message -> failure := Some message
                    | Ok value ->
                        bindings :=
                          (name, value) :: List.remove_assoc name !bindings;
                        blank_range rewritten start_byte end_byte
                    end
                | None ->
                    in_header := false;
                    if strict_control_flow_line trimmed then
                      saw_control_flow := true
              end
            else
              begin match standalone_assignment semantic_line with
              | Some _ when !saw_control_flow ->
                  failure :=
                    Some
                      "assignment after control flow requires chronological \
                       shell state"
              | Some (name, word) ->
                  if List.mem_assoc name !bindings then
                    failure :=
                      Some
                        "assignment after an executable command would mutate \
                         an existing shell binding"
                  else if parameter_mentioned_before source start_byte name then
                    failure :=
                      Some
                        "assignment after a prior parameter reference requires \
                         chronological shell state"
                  else
                    begin match expand_assignment_word !bindings word with
                    | Error message -> failure := Some message
                    | Ok value ->
                        bindings := (name, value) :: !bindings;
                        blank_range rewritten start_byte end_byte
                    end
              | None ->
                  if strict_control_flow_line trimmed then
                    saw_control_flow := true
              end;
            if !failure = None then
              match opened_heredoc with
              | Some heredoc -> active_heredoc := Some heredoc.delimiter
              | None -> ()))
    (line_ranges source);
  if not !saw_shell_options then None
  else
    Some
      (match !failure with
      | Some reason -> residual ~path ~source ~reason ()
      | None when (not !shell_options.errexit) || not !shell_options.nounset ->
          residual ~path ~source
            ~reason:
              "static option lowering requires both errexit (-e) and nounset \
               (-u)"
            ()
      | None ->
          begin match protect_literal_heredoc_dollars source rewritten with
          | Error reason -> residual ~path ~source ~reason ()
          | Ok literal_mappings ->
              begin match
                rewrite_command_parameters ~bindings:!bindings
                  (Bytes.to_string rewritten)
              with
              | Error reason -> residual ~path ~source ~reason ()
              | Ok (rewritten, mappings) ->
                  begin match
                    lower_strict_sequence ~path ~source:rewritten 0
                      (String.length rewritten)
                  with
                  | Error reason -> residual ~path ~source ~reason ()
                  | Ok root ->
                      let root =
                        map_template_node (literal_mappings @ mappings) root
                      in
                      let root =
                        Option.fold ~none:root
                          ~some:(fun directory ->
                            apply_working_directory directory root)
                          !working_directory
                      in
                      let contains_pipeline =
                        Ir.fold_nodes
                          (fun found node ->
                            found
                            ||
                            match node.Ir.operation with
                            | Ir.Pipeline _ -> true
                            | _ -> false)
                          false root
                      in
                      if !shell_options.pipefail && contains_pipeline then
                        residual ~path ~source
                          ~reason:
                            "pipefail rightmost-nonzero pipeline status is \
                             outside the current Effect IR"
                          ()
                      else { root; diagnostics = [] }
                  end
              end
          end)

let lower ~path source =
  match lower_strict_script ~path source with
  | Some result -> result
  | None -> (
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
          end)
