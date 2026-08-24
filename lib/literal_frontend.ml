type family = Fish | Powershell | Cmd | Nu
type token = { text : string; start_byte : int; end_byte : int }
type statement = { tokens : token list; source_start : int; source_end : int }

let interpreter = function
  | Fish -> "fish"
  | Powershell -> "powershell"
  | Cmd -> "cmd"
  | Nu -> "nu"

let basis = function
  | Fish -> "fish-explicit-external-command-v1"
  | Powershell -> "powershell-static-call-operator-v1"
  | Cmd -> "cmd-static-explicit-executable-v1"
  | Nu -> "nushell-static-external-command-v1"

let sequence_basis family = basis family ^ "-sequence"
let is_space = function ' ' | '\t' | '\r' -> true | _ -> false

let quote family character =
  match family with
  | Cmd -> character = '"'
  | Fish | Powershell | Nu -> character = '\'' || character = '"'

let dynamic family character =
  match family with
  | Fish -> List.mem character [ '$'; '`'; '*'; '?'; '['; ']'; '{'; '}'; '!' ]
  | Powershell ->
      List.mem character [ '$'; '`'; '*'; '?'; '['; ']'; '{'; '}'; '%'; '!' ]
  | Cmd ->
      List.mem character
        [ '$'; '`'; '*'; '?'; '['; ']'; '{'; '}'; '%'; '!'; '^' ]
  | Nu -> List.mem character [ '$'; '`'; '*'; '?'; '['; ']'; '{'; '}'; '%' ]

let native_expression_syntax family character =
  match family with
  | Fish -> List.mem character [ '('; ')'; '~'; '\\' ]
  | Powershell -> List.mem character [ '('; ')'; ','; '@' ]
  | Cmd -> List.mem character [ '('; ')' ]
  | Nu -> List.mem character [ '('; ')'; ','; '@' ]

let trim_bounds source start_byte end_byte =
  let start_byte = ref start_byte in
  let end_byte = ref end_byte in
  while !start_byte < !end_byte && is_space source.[!start_byte] do
    incr start_byte
  done;
  while !end_byte > !start_byte && is_space source.[!end_byte - 1] do
    decr end_byte
  done;
  (!start_byte, !end_byte)

let line_ranges source =
  let length = String.length source in
  let rec loop start index accumulator =
    if index = length then List.rev ((start, index) :: accumulator)
    else if source.[index] = '\n' then
      loop (index + 1) (index + 1) ((start, index) :: accumulator)
    else loop start (index + 1) accumulator
  in
  if length = 0 then [] else loop 0 0 []

let lowercase_range source start_byte end_byte =
  String.sub source start_byte (end_byte - start_byte) |> String.lowercase_ascii

let cmd_line_kind source start_byte end_byte =
  let value = lowercase_range source start_byte end_byte |> String.trim in
  if value = "@echo off" then `Echo_off
  else
    let without_at =
      if String.starts_with ~prefix:"@" value then
        String.sub value 1 (String.length value - 1) |> String.trim
      else value
    in
    if
      String.starts_with ~prefix:"rem " without_at
      || without_at = "rem"
      || String.starts_with ~prefix:"::" without_at
    then `Comment
    else `Command

let comment_line family source start_byte end_byte =
  start_byte = end_byte
  ||
  match family with
  | Cmd -> false
  | Fish | Powershell | Nu -> source.[start_byte] = '#'

let lex_statement family source ~start_byte ~end_byte =
  let buffer = Buffer.create 32 in
  let tokens = ref [] in
  let token_start = ref None in
  let state = ref `Normal in
  let failure = ref None in
  let index = ref start_byte in
  let start offset = if !token_start = None then token_start := Some offset in
  let add offset character =
    start offset;
    Buffer.add_char buffer character
  in
  let flush finish =
    match !token_start with
    | None -> ()
    | Some token_start_byte ->
        tokens :=
          {
            text = Buffer.contents buffer;
            start_byte = token_start_byte;
            end_byte = finish;
          }
          :: !tokens;
        Buffer.clear buffer;
        token_start := None
  in
  while !index < end_byte && !failure = None do
    let character = source.[!index] in
    begin match !state with
    | `Quoted delimiter ->
        if character = delimiter then
          if !index + 1 < end_byte && source.[!index + 1] = delimiter then
            if family = Powershell && delimiter = '\'' then begin
              add !index delimiter;
              incr index
            end
            else
              failure :=
                Some "quoted escape syntax is outside the literal argv subset"
          else state := `Normal
        else if
          character = '\\' && (family = Fish || (family = Nu && delimiter = '"'))
        then
          failure :=
            Some "quoted escape syntax is outside the literal argv subset"
        else if dynamic family character then
          failure := Some "dynamic expansion is outside the static subset"
        else add !index character
    | `Normal ->
        if is_space character then flush !index
        else if quote family character then begin
          start !index;
          state := `Quoted character
        end
        else
          begin match character with
          | '#' when family <> Cmd && !token_start = None ->
              flush !index;
              index := end_byte - 1
          | '&' when family = Powershell && !tokens = [] && !token_start = None
            ->
              add !index character
          | '^' when family = Nu && !tokens = [] && !token_start = None ->
              add !index character
          | '&' | '|' | ';' | '<' | '>' ->
              failure :=
                Some "shell control operators require a residual capsule"
          | character when native_expression_syntax family character ->
              failure :=
                Some
                  "native expression syntax is outside the literal argv subset"
          | character when dynamic family character ->
              failure := Some "dynamic expansion is outside the static subset"
          | character -> add !index character
          end
    end;
    incr index
  done;
  match (!failure, !state) with
  | Some message, _ -> Error message
  | None, `Quoted _ -> Error "unterminated quoted argument"
  | None, `Normal ->
      flush end_byte;
      Ok (List.rev !tokens)

let statements family source =
  let rec collect cmd_echo_off accumulator = function
    | [] -> Ok (List.rev accumulator)
    | (line_start, line_end) :: rest ->
        let start_byte, end_byte = trim_bounds source line_start line_end in
        if comment_line family source start_byte end_byte then
          collect cmd_echo_off accumulator rest
        else if family = Cmd then
          begin match cmd_line_kind source start_byte end_byte with
          | `Echo_off -> collect true accumulator rest
          | `Comment ->
              if cmd_echo_off || source.[start_byte] = '@' then
                collect cmd_echo_off accumulator rest
              else
                Error
                  "cmd command echo must be suppressed before comments can be \
                   lowered"
          | `Command ->
              let local_echo_suppressed = source.[start_byte] = '@' in
              if not (cmd_echo_off || local_echo_suppressed) then
                Error
                  "cmd command echo must be suppressed with @echo off or an @ \
                   command prefix"
              else
                let command_start =
                  if local_echo_suppressed then start_byte + 1 else start_byte
                in
                begin match
                  lex_statement family source ~start_byte:command_start
                    ~end_byte
                with
                | Error _ as error -> error
                | Ok [] -> collect cmd_echo_off accumulator rest
                | Ok tokens ->
                    collect cmd_echo_off
                      ({
                         tokens;
                         source_start = start_byte;
                         source_end = end_byte;
                       }
                      :: accumulator)
                      rest
                end
          end
        else
          begin match lex_statement family source ~start_byte ~end_byte with
          | Error _ as error -> error
          | Ok [] -> collect cmd_echo_off accumulator rest
          | Ok tokens ->
              collect cmd_echo_off
                ({ tokens; source_start = start_byte; source_end = end_byte }
                :: accumulator)
                rest
          end
  in
  collect false [] (line_ranges source)

let valid_powershell_name value =
  value <> ""
  &&
  match value.[0] with
  | 'A' .. 'Z' | 'a' .. 'z' | '_' ->
      String.for_all
        (function
          | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> true | _ -> false)
        value
  | _ -> false

let blank_range bytes start_byte end_byte =
  for index = start_byte to end_byte - 1 do
    if Bytes.get bytes index <> '\n' then Bytes.set bytes index ' '
  done

let template_literal value =
  Posix_frontend.replace_all value ~pattern:"$" ~replacement:"$$"

let powershell_reference bindings expression =
  let lower = String.lowercase_ascii expression in
  if String.starts_with ~prefix:"env:" lower then
    let name = String.sub expression 4 (String.length expression - 4) in
    if valid_powershell_name name then Ok ("${" ^ name ^ "}")
    else Error ("invalid PowerShell environment variable: " ^ expression)
  else if valid_powershell_name expression then
    match List.assoc_opt lower bindings with
    | Some value -> Ok value
    | None -> Error ("unbound PowerShell variable: $" ^ expression)
  else Error ("unsupported PowerShell variable: $" ^ expression)

let parse_powershell_reference bindings source index end_byte =
  if index + 1 >= end_byte then Error "trailing PowerShell variable marker"
  else if source.[index + 1] = '(' then
    Error "PowerShell subexpressions are outside the static subset"
  else if source.[index + 1] = '{' then
    match String.index_from_opt source (index + 2) '}' with
    | None -> Error "unterminated PowerShell braced variable"
    | Some close when close >= end_byte ->
        Error "unterminated PowerShell braced variable"
    | Some close ->
        let expression = String.sub source (index + 2) (close - index - 2) in
        Result.map
          (fun value -> (close + 1, value))
          (powershell_reference bindings expression)
  else
    let rec finish cursor =
      if cursor >= end_byte then cursor
      else
        match source.[cursor] with
        | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' | ':' -> finish (cursor + 1)
        | _ -> cursor
    in
    let finish = finish (index + 1) in
    if finish = index + 1 then Error "unsupported PowerShell special variable"
    else
      let expression = String.sub source (index + 1) (finish - index - 1) in
      Result.map
        (fun value -> (finish, value))
        (powershell_reference bindings expression)

let expand_powershell_string bindings value =
  let output = Buffer.create (String.length value) in
  let failure = ref None in
  let index = ref 0 in
  while !index < String.length value && !failure = None do
    match value.[!index] with
    | '`' ->
        failure :=
          Some "PowerShell escape sequences are outside the static subset"
    | '$' ->
        begin match
          parse_powershell_reference bindings value !index (String.length value)
        with
        | Error message -> failure := Some message
        | Ok (finish, replacement) ->
            Buffer.add_string output replacement;
            index := finish - 1
        end
    | character ->
        Buffer.add_char output character;
        incr index
  done;
  match !failure with
  | Some message -> Error message
  | None -> Ok (Buffer.contents output)

let decode_powershell_single_quoted value =
  let output = Buffer.create (String.length value) in
  let rec loop index =
    if index >= String.length value then Ok (Buffer.contents output)
    else if value.[index] <> '\'' then begin
      Buffer.add_char output value.[index];
      loop (index + 1)
    end
    else if index + 1 < String.length value && value.[index + 1] = '\'' then begin
      Buffer.add_char output '\'';
      loop (index + 2)
    end
    else Error "PowerShell single-quote escaping is outside the static subset"
  in
  loop 0

let powershell_assignment bindings line =
  match String.index_opt line '=' with
  | None -> Error "PowerShell assignment is missing ="
  | Some separator ->
      let left = String.sub line 0 separator |> String.trim in
      let right =
        String.sub line (separator + 1) (String.length line - separator - 1)
        |> String.trim
      in
      if
        String.length left < 2
        || left.[0] <> '$'
        || not
             (valid_powershell_name
                (String.sub left 1 (String.length left - 1)))
      then Error "PowerShell assignment target is outside the static subset"
      else
        let name = String.sub left 1 (String.length left - 1) in
        let key = String.lowercase_ascii name in
        let value =
          let length = String.length right in
          if length >= 2 && right.[0] = '\'' && right.[length - 1] = '\'' then
            let inner = String.sub right 1 (length - 2) in
            Result.map template_literal (decode_powershell_single_quoted inner)
          else if length >= 2 && right.[0] = '"' && right.[length - 1] = '"'
          then
            String.sub right 1 (length - 2) |> expand_powershell_string bindings
          else if String.starts_with ~prefix:"$" right then
            begin match
              parse_powershell_reference bindings right 0 (String.length right)
            with
            | Ok (finish, value) when finish = String.length right -> Ok value
            | Ok _ ->
                Error
                  "PowerShell assignment expression has unsupported trailing \
                   syntax"
            | Error _ as error -> error
            end
          else Error "PowerShell assignment value is outside the static subset"
        in
        Result.map (fun value -> (key, name, value)) value

let marker_for source mappings index length =
  let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ" in
  let rec candidate nonce =
    if nonce >= 26 * 26 then None
    else
      let value =
        String.init length (fun position ->
            alphabet.[(nonce + (position * 7) + (index * 11)) mod 26])
      in
      if Posix_frontend.contains source value || List.mem_assoc value mappings
      then candidate (nonce + 1)
      else Some value
  in
  candidate 0

let rewrite_powershell_variables bindings source =
  let bytes = Bytes.of_string source in
  let mappings = ref [] in
  let state = ref `Normal in
  let failure = ref None in
  let index = ref 0 in
  let add_mapping start_byte end_byte replacement =
    let length = end_byte - start_byte in
    match marker_for source !mappings (List.length !mappings) length with
    | None -> failure := Some "too many PowerShell references to map safely"
    | Some marker ->
        Bytes.blit_string marker 0 bytes start_byte length;
        mappings := (marker, replacement) :: !mappings;
        index := end_byte - 1
  in
  while !index < String.length source && !failure = None do
    let character = source.[!index] in
    begin match !state with
    | `Comment -> if character = '\n' then state := `Normal
    | `Single ->
        if character = '\'' then state := `Normal
        else if character = '$' then add_mapping !index (!index + 1) "$$"
    | `Double ->
        if character = '"' then state := `Normal
        else if character = '`' then
          failure :=
            Some "PowerShell escape sequences are outside the static subset"
        else if character = '$' then
          begin match
            parse_powershell_reference bindings source !index
              (String.length source)
          with
          | Error message -> failure := Some message
          | Ok (finish, replacement) -> add_mapping !index finish replacement
          end
    | `Normal ->
        begin match character with
        | '#' -> state := `Comment
        | '\'' -> state := `Single
        | '"' -> state := `Double
        | '`' ->
            failure :=
              Some "PowerShell escape sequences are outside the static subset"
        | '$' ->
            begin match
              parse_powershell_reference bindings source !index
                (String.length source)
            with
            | Error message -> failure := Some message
            | Ok (finish, _)
              when finish < String.length source
                   && List.mem source.[finish] [ '.'; '[' ] ->
                failure :=
                  Some
                    "PowerShell member and index access are outside the static \
                     value subset"
            | Ok (finish, replacement) -> add_mapping !index finish replacement
            end
        | _ -> ()
        end
    end;
    incr index
  done;
  match (!failure, !state) with
  | Some message, _ -> Error message
  | None, (`Single | `Double) -> Error "unterminated PowerShell string"
  | None, (`Normal | `Comment) -> Ok (Bytes.to_string bytes, List.rev !mappings)

let powershell_parameter_mentioned_before source end_byte name =
  let prefix = String.sub source 0 end_byte |> String.lowercase_ascii in
  let name = String.lowercase_ascii name in
  Posix_frontend.contains prefix ("$" ^ name)
  || Posix_frontend.contains prefix ("${" ^ name)

let mask_powershell_block_comments source =
  let bytes = Bytes.of_string source in
  let state = ref `Normal in
  let index = ref 0 in
  let blank offset =
    if source.[offset] <> '\n' then Bytes.set bytes offset ' '
  in
  while !index < String.length source do
    let character = source.[!index] in
    begin match !state with
    | `Line_comment -> if character = '\n' then state := `Normal
    | `Single ->
        if character = '\'' then
          if !index + 1 < String.length source && source.[!index + 1] = '\''
          then incr index
          else state := `Normal
    | `Double ->
        if character = '`' && !index + 1 < String.length source then incr index
        else if character = '"' then state := `Normal
    | `Block_comment ->
        blank !index;
        if
          character = '#'
          && !index + 1 < String.length source
          && source.[!index + 1] = '>'
        then begin
          incr index;
          blank !index;
          state := `Normal
        end
    | `Normal ->
        if character = '#' then state := `Line_comment
        else if character = '\'' then state := `Single
        else if character = '"' then state := `Double
        else if
          character = '<'
          && !index + 1 < String.length source
          && source.[!index + 1] = '#'
        then begin
          blank !index;
          incr index;
          blank !index;
          state := `Block_comment
        end
    end;
    incr index
  done;
  match !state with
  | `Block_comment -> Error "unterminated PowerShell block comment"
  | `Normal | `Line_comment | `Single | `Double -> Ok (Bytes.to_string bytes)

let compact_powershell_header value =
  value |> String.lowercase_ascii |> String.to_seq
  |> Seq.filter (function ' ' | '\t' | '\r' -> false | _ -> true)
  |> String.of_seq

let preprocess_powershell source =
  match mask_powershell_block_comments source with
  | Error _ as error -> error
  | Ok masked_source -> (
      let rewritten = Bytes.of_string masked_source in
      let bindings = ref [] in
      let in_header = ref true in
      let failure = ref None in
      List.iter
        (fun (start_byte, end_byte) ->
          if !failure = None then
            let line =
              String.sub masked_source start_byte (end_byte - start_byte)
              |> String.trim
            in
            let compact_header = compact_powershell_header line in
            if line = "" || String.starts_with ~prefix:"#" line then ()
            else if !in_header && compact_header = "[cmdletbinding()]" then
              blank_range rewritten start_byte end_byte
            else if !in_header && compact_header = "param()" then
              blank_range rewritten start_byte end_byte
            else if
              compact_header <> "param()"
              && String.starts_with ~prefix:"param(" compact_header
            then
              failure :=
                Some "PowerShell parameter blocks require typed input lowering"
            else if
              !in_header && compact_header = "set-strictmode-versionlatest"
            then blank_range rewritten start_byte end_byte
            else if
              String.starts_with ~prefix:"$" line && String.contains line '='
            then
              begin match powershell_assignment !bindings line with
              | Error message -> failure := Some message
              | Ok (key, name, value) ->
                  if key = "psnativecommanduseerroractionpreference" then
                    failure :=
                      Some
                        "$PSNativeCommandUseErrorActionPreference changes \
                         native failure semantics"
                  else if key = "erroractionpreference" then
                    if not !in_header then
                      failure :=
                        Some
                          "$ErrorActionPreference after execution changes \
                           error semantics"
                    else if String.lowercase_ascii value = "stop" then
                      blank_range rewritten start_byte end_byte
                    else
                      failure :=
                        Some "$ErrorActionPreference must be the literal 'Stop'"
                  else if (not !in_header) && List.mem_assoc key !bindings then
                    failure :=
                      Some
                        "PowerShell assignment after execution would mutate an \
                         existing binding"
                  else if
                    (not !in_header)
                    && powershell_parameter_mentioned_before source start_byte
                         name
                  then
                    failure :=
                      Some
                        "PowerShell assignment after a prior reference \
                         requires chronological state"
                  else begin
                    bindings := (key, value) :: List.remove_assoc key !bindings;
                    blank_range rewritten start_byte end_byte
                  end
              end
            else in_header := false)
        (line_ranges masked_source);
      match !failure with
      | Some message -> Error message
      | None ->
          rewrite_powershell_variables !bindings (Bytes.to_string rewritten))

let basename executable =
  executable
  |> String.map (function '\\' -> '/' | character -> character)
  |> String.split_on_char '/' |> List.rev
  |> function
  | name :: _ -> name
  | [] -> executable

let explicit_cmd_executable value =
  let name = basename value |> String.lowercase_ascii in
  Filename.check_suffix name ".exe" || Filename.check_suffix name ".com"

let external_argv family tokens =
  match (family, tokens) with
  | Fish, { text = "command"; _ } :: ({ text = executable; _ } :: _ as rest)
    when executable <> "" ->
      Ok rest
  | Powershell, { text = "&"; _ } :: ({ text = executable; _ } :: _ as rest)
    when executable <> "" ->
      Ok rest
  | Cmd, ({ text = executable; _ } :: _ as values)
    when explicit_cmd_executable executable ->
      Ok values
  | Nu, { text; start_byte; end_byte } :: rest
    when String.length text > 1 && text.[0] = '^' ->
      Ok
        ({
           text = String.sub text 1 (String.length text - 1);
           start_byte;
           end_byte;
         }
        :: rest)
  | _ ->
      Error
        (Printf.sprintf
           "%s command is not an explicit static external invocation"
           (interpreter family))

let lower family ~path source =
  let residual reason =
    Posix_frontend.residual ~interpreter:(interpreter family) ~path ~source
      ~reason ()
  in
  let preprocessed =
    match family with
    | Powershell -> preprocess_powershell source
    | Fish | Cmd | Nu -> Ok (source, [])
  in
  let rec lower_statements index accumulator = function
    | [] when accumulator = [] ->
        Error "script contains no static external invocation"
    | [] -> Ok (List.rev accumulator)
    | statement :: rest ->
        begin match external_argv family statement.tokens with
        | Error _ as error -> error
        | Ok command_tokens ->
            let argv = List.map (fun token -> token.text) command_tokens in
            let span =
              Posix_frontend.span_for_range ~path source
                ~start_byte:statement.source_start
                ~end_byte:statement.source_end
            in
            let node =
              Ir.node
                ~id:
                  (Posix_frontend.make_id ~path ~index
                     (interpreter family ^ "\000" ^ String.concat "\000" argv))
                ~guarantee:(Ir.Formal { basis = basis family })
                ~source:span
                (Ir.Exec (Ir.exec argv))
            in
            lower_statements (index + 1) (node :: accumulator) rest
        end
  in
  match preprocessed with
  | Error reason -> residual reason
  | Ok (lowered_source, mappings) ->
      begin match statements family lowered_source with
      | Error reason -> residual reason
      | Ok (_ :: _ :: _) when family = Nu ->
          residual
            "multiple Nushell statements require a pinned runtime status \
             contract"
      | Ok statements ->
          begin match lower_statements 0 [] statements with
          | Error reason -> residual reason
          | Ok nodes ->
              let root =
                match nodes with
                | [ root ] -> root
                | nodes ->
                    let first_span = Option.get (List.hd nodes).Ir.source in
                    let last_span =
                      Option.get (List.hd (List.rev nodes)).Ir.source
                    in
                    Ir.node
                      ~id:
                        (Posix_frontend.make_id ~path ~index:50_000
                           (interpreter family ^ "\000sequence\000" ^ source))
                      ~guarantee:(Ir.Formal { basis = sequence_basis family })
                      ~source:(Posix_frontend.cover_spans first_span last_span)
                      (Ir.Sequence nodes)
              in
              let root = Posix_frontend.map_template_node mappings root in
              let root =
                if family <> Powershell then root
                else
                  let commands =
                    match root.Ir.operation with
                    | Ir.Sequence nodes -> nodes
                    | _ -> [ root ]
                  in
                  let completion_offset =
                    match root.source with
                    | Some span -> span.Ir.end_byte
                    | None -> String.length source
                  in
                  let completion =
                    Ir.node
                      ~id:
                        (Posix_frontend.make_id ~path ~index:59_999
                           "powershell-normal-completion")
                      ~guarantee:
                        (Ir.Formal
                           { basis = "powershell-file-normal-completion-v1" })
                      ~source:
                        (Posix_frontend.span_for_range ~path source
                           ~start_byte:completion_offset
                           ~end_byte:completion_offset)
                      (Ir.Sequence [])
                  in
                  Ir.node ?source:root.source
                    ~id:
                      (Posix_frontend.make_id ~path ~index:60_000
                         ("powershell-file\000" ^ source))
                    ~guarantee:
                      (Ir.Formal
                         { basis = "powershell-file-normal-completion-v1" })
                    (Ir.Sequence (commands @ [ completion ]))
              in
              Posix_frontend.{ root; diagnostics = [] }
          end
      end
