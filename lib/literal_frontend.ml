type family = Fish | Powershell | Cmd | Nu
type token = { text : string; start_byte : int; end_byte : int }

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

let is_space = function ' ' | '\t' | '\r' -> true | _ -> false

let lex family source =
  let length = String.length source in
  let buffer = Buffer.create 32 in
  let tokens = ref [] in
  let token_start = ref None in
  let state = ref `Normal in
  let failure = ref None in
  let index = ref 0 in
  let start offset = if !token_start = None then token_start := Some offset in
  let add offset character =
    start offset;
    Buffer.add_char buffer character
  in
  let flush end_byte =
    match !token_start with
    | None -> ()
    | Some start_byte ->
        tokens :=
          { text = Buffer.contents buffer; start_byte; end_byte } :: !tokens;
        Buffer.clear buffer;
        token_start := None
  in
  let dynamic character =
    match character with
    | '$' | '`' | '*' | '?' | '[' | ']' | '{' | '}' | '%' | '!' -> true
    | _ -> false
  in
  while !index < length && !failure = None do
    let character = source.[!index] in
    begin match !state with
    | `Single ->
        if character = '\'' then state := `Normal
        else if dynamic character then
          failure := Some "dynamic expansion is outside the static subset"
        else add !index character
    | `Double ->
        if character = '"' then state := `Normal
        else if dynamic character then
          failure := Some "dynamic expansion is outside the static subset"
        else add !index character
    | `Normal ->
        if is_space character then flush !index
        else
          begin match character with
          | '\n' ->
              let remainder =
                String.sub source (!index + 1) (length - !index - 1)
              in
              if String.trim remainder = "" then begin
                flush !index;
                index := length - 1
              end
              else
                failure := Some "multiple statements require a residual capsule"
          | '\'' ->
              start !index;
              state := `Single
          | '"' ->
              start !index;
              state := `Double
          | '&' when family = Powershell && !tokens = [] && !token_start = None
            ->
              add !index character
          | '^' when family = Nu && !tokens = [] && !token_start = None ->
              add !index character
          | '&' | '|' | ';' | '<' | '>' ->
              failure :=
                Some "shell control operators require a residual capsule"
          | character when dynamic character ->
              failure := Some "dynamic expansion is outside the static subset"
          | character -> add !index character
          end
    end;
    incr index
  done;
  match (!failure, !state) with
  | Some message, _ -> Error message
  | None, (`Single | `Double) -> Error "unterminated quoted argument"
  | None, `Normal ->
      flush length;
      Ok (List.rev !tokens)

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
  match lex family source with
  | Error reason -> residual reason
  | Ok tokens ->
      begin match external_argv family tokens with
      | Error reason -> residual reason
      | Ok command_tokens ->
          let first = List.hd tokens in
          let last = List.hd (List.rev tokens) in
          let span =
            Posix_frontend.span_for_range ~path source
              ~start_byte:first.start_byte ~end_byte:last.end_byte
          in
          let argv = List.map (fun token -> token.text) command_tokens in
          let root =
            Ir.node
              ~id:
                (Posix_frontend.make_id ~path ~index:0
                   (interpreter family ^ "\000" ^ String.concat "\000" argv))
              ~guarantee:(Ir.Formal { basis = basis family })
              ~source:span
              (Ir.Exec (Ir.exec argv))
          in
          Posix_frontend.{ root; diagnostics = [] }
      end
