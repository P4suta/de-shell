(* Unpublished OCaml reference for the deterministic Effect IR v1 golden gate. *)

open Yojson.Safe

type text_part = Literal of string | Variable of string | Argument of string
type text_expression = text_part list

type span = {
  file : string;
  start_line : int;
  start_column : int;
  end_line : int;
  end_column : int;
  start_byte : int;
  end_byte : int;
}

type guarantee = Native of string | Delegated of string | Residual of string
type source_bytes = Utf8 of string | Base64 of string

type operation =
  | Exec of text_expression list
  | Sequence of node list
  | Set_variable of {
      name : string;
      value_type : string;
      value : text_expression;
    }
  | Interpreter_call of {
      interpreter : string;
      interpreter_pin : string;
      source : source_bytes;
      source_span : span;
      capabilities : string list;
      reason : string;
    }
  | Opaque_capsule of {
      interpreter : string;
      source : source_bytes;
      path : string;
    }

and node = {
  operation : operation;
  guarantee : guarantee;
  source : span option;
}

type lowered = { body : node; environment : string list }

type golden_case = {
  name : string;
  path : string;
  source_utf8 : string option;
  source_base64 : string option;
  root_operation : string;
  native : int;
  delegated : int;
  residual : int;
  plan_digest : string;
}

let default_corpus = "contracts/golden/frontend-v1.json"
let default_transform_corpus = "contracts/golden/transform-export-v1.json"
let fail format = Printf.ksprintf failwith format

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let rec canonicalize = function
  | `Assoc fields ->
      `Assoc
        (fields
        |> List.map (fun (name, value) -> (name, canonicalize value))
        |> List.sort (fun (left, _) (right, _) -> String.compare left right))
  | `List values -> `List (List.map canonicalize values)
  | value -> value

let canonical_string value = Yojson.Safe.to_string (canonicalize value)

let pretty_string value =
  let output = Buffer.create 256 in
  let indent depth = Buffer.add_string output (String.make (depth * 2) ' ') in
  let rec write depth = function
    | `Assoc fields ->
        let fields =
          fields
          |> List.map (fun (name, value) -> (name, canonicalize value))
          |> List.sort (fun (left, _) (right, _) -> String.compare left right)
        in
        Buffer.add_char output '{';
        if fields <> [] then begin
          fields
          |> List.iteri (fun index (name, value) ->
              if index > 0 then Buffer.add_char output ',';
              Buffer.add_char output '\n';
              indent (depth + 1);
              Buffer.add_string output (Yojson.Safe.to_string (`String name));
              Buffer.add_string output ": ";
              write (depth + 1) value);
          Buffer.add_char output '\n';
          indent depth
        end;
        Buffer.add_char output '}'
    | `List values ->
        Buffer.add_char output '[';
        if values <> [] then begin
          values
          |> List.iteri (fun index value ->
              if index > 0 then Buffer.add_char output ',';
              Buffer.add_char output '\n';
              indent (depth + 1);
              write (depth + 1) value);
          Buffer.add_char output '\n';
          indent depth
        end;
        Buffer.add_char output ']'
    | value -> Buffer.add_string output (Yojson.Safe.to_string value)
  in
  write 0 (canonicalize value);
  Buffer.add_char output '\n';
  Buffer.contents output

let int64_be value =
  String.init 8 (fun index ->
      let shift = (7 - index) * 8 in
      Int64.(to_int (logand (shift_right_logical value shift) 0xffL))
      |> Char.chr)

let framed value = int64_be (Int64.of_int (String.length value)) ^ value

let operation_name = function
  | Exec _ -> "exec"
  | Sequence _ -> "sequence"
  | Set_variable _ -> "set_variable"
  | Interpreter_call _ -> "interpreter_call"
  | Opaque_capsule _ -> "opaque_capsule"

let node_id node preorder =
  let path, start_byte, end_byte =
    match node.source with
    | Some source -> (source.file, source.start_byte, source.end_byte)
    | None -> ("", 0, 0)
  in
  let operation = operation_name node.operation in
  let input =
    "deshell.node-id.v1\000" ^ framed path ^ framed operation
    ^ int64_be (Int64.of_int start_byte)
    ^ int64_be (Int64.of_int end_byte)
    ^ int64_be (Int64.of_int preorder)
  in
  String.sub (Sha256.hex input) 0 32

let json_part = function
  | Literal value ->
      `Assoc [ ("kind", `String "literal"); ("value", `String value) ]
  | Variable name ->
      `Assoc [ ("kind", `String "variable"); ("name", `String name) ]
  | Argument name ->
      `Assoc [ ("kind", `String "argument"); ("name", `String name) ]

let json_expression parts =
  `Assoc [ ("parts", `List (List.map json_part parts)) ]

let json_span source =
  `Assoc
    [
      ("file", `String source.file);
      ("start_line", `Int source.start_line);
      ("start_column", `Int source.start_column);
      ("end_line", `Int source.end_line);
      ("end_column", `Int source.end_column);
      ("start_byte", `Int source.start_byte);
      ("end_byte", `Int source.end_byte);
    ]

let json_guarantee = function
  | Native semantic_model ->
      `Assoc
        [
          ("level", `String "native"); ("semantic_model", `String semantic_model);
        ]
  | Delegated reason ->
      `Assoc [ ("level", `String "delegated"); ("reason", `String reason) ]
  | Residual reason ->
      `Assoc [ ("level", `String "residual"); ("reason", `String reason) ]

let json_source_bytes = function
  | Utf8 text -> `Assoc [ ("encoding", `String "utf8"); ("text", `String text) ]
  | Base64 base64 ->
      `Assoc [ ("encoding", `String "base64"); ("base64", `String base64) ]

let rec json_node preorder node =
  let current = !preorder in
  incr preorder;
  let operation =
    match node.operation with
    | Exec argv ->
        `Assoc
          [
            ("type", `String "exec");
            ("argv", `List (List.map json_expression argv));
            ("environment", `List []);
            ("working_directory", `Null);
          ]
    | Sequence nodes ->
        `Assoc
          [
            ("type", `String "sequence");
            ("nodes", `List (List.map (json_node preorder) nodes));
          ]
    | Set_variable { name; value_type; value } ->
        `Assoc
          [
            ("type", `String "set_variable");
            ("name", `String name);
            ("value_type", `String value_type);
            ("value", json_expression value);
          ]
    | Interpreter_call
        {
          interpreter;
          interpreter_pin;
          source;
          source_span;
          capabilities;
          reason;
        } ->
        `Assoc
          [
            ("type", `String "interpreter_call");
            ("interpreter", `String interpreter);
            ("interpreter_pin", `String interpreter_pin);
            ("source", json_source_bytes source);
            ("source_span", json_span source_span);
            ( "capabilities",
              `List (List.map (fun value -> `String value) capabilities) );
            ("reason", `String reason);
          ]
    | Opaque_capsule { interpreter; source; path } ->
        `Assoc
          [
            ("type", `String "opaque_capsule");
            ("interpreter", `String interpreter);
            ("source", json_source_bytes source);
            ("path", `String path);
          ]
  in
  `Assoc
    [
      ("id", `String (node_id node current));
      ("operation", operation);
      ("guarantee", json_guarantee node.guarantee);
      ("source", Option.fold ~none:`Null ~some:json_span node.source);
    ]

let json_plan lowered =
  let preorder = ref 0 in
  `Assoc
    [
      ("schema_version", `Int 1);
      ("generator", `String "deshell/0.1.0");
      ("entrypoint", `String "main");
      ( "tasks",
        `List
          [
            `Assoc
              [
                ("name", `String "main");
                ("inputs", `List []);
                ("outputs", `List []);
                ( "environment",
                  `List
                    (List.map (fun value -> `String value) lowered.environment)
                );
                ("secrets", `List []);
                ("platform_capabilities", `List []);
                ("cacheable", `Bool false);
                ("invocation", `Null);
                ("body", json_node preorder lowered.body);
              ];
          ] );
    ]

let scalar_position source offset =
  let line = ref 1 in
  let column = ref 0 in
  for index = 0 to offset - 1 do
    let byte = Char.code source.[index] in
    if byte = 0x0a then begin
      incr line;
      column := 0
    end
    else if byte land 0xc0 <> 0x80 then incr column
  done;
  (!line, !column)

let source_span path source start_byte end_byte =
  let start_line, start_column = scalar_position source start_byte in
  let end_line, end_column = scalar_position source end_byte in
  {
    file = path;
    start_line;
    start_column;
    end_line;
    end_column;
    start_byte;
    end_byte;
  }

let is_space = function ' ' | '\t' -> true | _ -> false

let is_name = function
  | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> true
  | _ -> false

let tokenize source =
  let length = String.length source in
  let tokens = ref [] in
  let parts = ref [] in
  let literal = Buffer.create 16 in
  let active = ref false in
  let flush_literal () =
    if Buffer.length literal > 0 then begin
      parts := Literal (Buffer.contents literal) :: !parts;
      Buffer.clear literal
    end
  in
  let flush_token () =
    flush_literal ();
    if !active then begin
      let value = List.rev !parts in
      tokens := (if value = [] then [ Literal "" ] else value) :: !tokens;
      parts := [];
      active := false
    end
  in
  let index = ref 0 in
  let quote = ref None in
  while !index < length do
    let current = source.[!index] in
    match !quote with
    | Some '\'' ->
        active := true;
        if current = '\'' then quote := None
        else Buffer.add_char literal current;
        incr index
    | Some '"' ->
        active := true;
        if current = '"' then begin
          quote := None;
          incr index
        end
        else if current = '$' then begin
          flush_literal ();
          let finish = ref (!index + 1) in
          while !finish < length && is_name source.[!finish] do
            incr finish
          done;
          if !finish = !index + 1 then Buffer.add_char literal '$'
          else
            parts :=
              Variable (String.sub source (!index + 1) (!finish - !index - 1))
              :: !parts;
          index := !finish
        end
        else begin
          Buffer.add_char literal current;
          incr index
        end
    | Some _ -> assert false
    | None ->
        if is_space current then begin
          flush_token ();
          incr index
        end
        else if current = '\'' || current = '"' then begin
          active := true;
          quote := Some current;
          incr index
        end
        else if current = '$' then begin
          active := true;
          flush_literal ();
          let finish = ref (!index + 1) in
          while !finish < length && is_name source.[!finish] do
            incr finish
          done;
          if !finish = !index + 1 then Buffer.add_char literal '$'
          else
            parts :=
              Variable (String.sub source (!index + 1) (!finish - !index - 1))
              :: !parts;
          index := !finish
        end
        else if current = '\\' && !index + 1 < length then begin
          active := true;
          Buffer.add_char literal source.[!index + 1];
          index := !index + 2
        end
        else begin
          active := true;
          Buffer.add_char literal current;
          incr index
        end
  done;
  if !quote <> None then fail "unterminated quote";
  flush_token ();
  List.rev !tokens

let trim_range source start_byte end_byte =
  let start_byte = ref start_byte in
  let end_byte = ref end_byte in
  while !start_byte < !end_byte && is_space source.[!start_byte] do
    incr start_byte
  done;
  while
    !end_byte > !start_byte
    && (is_space source.[!end_byte - 1] || source.[!end_byte - 1] = '\r')
  do
    decr end_byte
  done;
  (!start_byte, !end_byte)

let line_ranges source =
  let ranges = ref [] in
  let start = ref 0 in
  for index = 0 to String.length source - 1 do
    if source.[index] = '\n' then begin
      let first, last = trim_range source !start index in
      if first < last then ranges := (first, last) :: !ranges;
      start := index + 1
    end
  done;
  if !start < String.length source then begin
    let first, last = trim_range source !start (String.length source) in
    if first < last then ranges := (first, last) :: !ranges
  end;
  List.rev !ranges

let variables expressions =
  expressions |> List.concat
  |> List.filter_map (function Variable name -> Some name | _ -> None)
  |> List.sort_uniq String.compare

let lower_posix path source interpreter =
  let locals = ref [] in
  let environment = ref [] in
  let nodes =
    line_ranges source
    |> List.filter_map (fun (start_byte, end_byte) ->
        let line = String.sub source start_byte (end_byte - start_byte) in
        if String.starts_with ~prefix:"#!" line then None
        else
          match String.index_opt line '=' with
          | Some equals
            when equals > 0
                 && not (String.exists is_space (String.sub line 0 equals)) ->
              let name = String.sub line 0 equals in
              let value =
                String.sub line (equals + 1) (String.length line - equals - 1)
              in
              locals := name :: !locals;
              let value_type =
                match Int64.of_string_opt value with
                | Some _ -> "int"
                | None -> "text"
              in
              Some
                {
                  operation =
                    Set_variable { name; value_type; value = [ Literal value ] };
                  guarantee = Native "posix-immutable-assignment-v1";
                  source = Some (source_span path source start_byte end_byte);
                }
          | _ ->
              let argv = tokenize line in
              environment := variables argv @ !environment;
              Some
                {
                  operation = Exec argv;
                  guarantee = Native (interpreter ^ "-explicit-command-v1");
                  source = Some (source_span path source start_byte end_byte);
                })
  in
  match nodes with
  | [] -> fail "no lowerable POSIX operation"
  | [ body ] ->
      {
        body;
        environment =
          !environment
          |> List.filter (fun name -> not (List.mem name !locals))
          |> List.sort_uniq String.compare;
      }
  | first :: _ ->
      let last = List.hd (List.rev nodes) in
      let first_span = Option.get first.source in
      let last_span = Option.get last.source in
      let source =
        source_span path source first_span.start_byte last_span.end_byte
      in
      {
        body =
          {
            operation = Sequence nodes;
            guarantee = Native (interpreter ^ "-static-sequence-v1");
            source = Some source;
          };
        environment =
          !environment
          |> List.filter (fun name -> not (List.mem name !locals))
          |> List.sort_uniq String.compare;
      }

let rewrite_first_literal transform = function
  | (Literal value :: rest) :: expressions ->
      (Literal (transform value) :: rest) :: expressions
  | _ -> fail "external command must start with a literal executable"

let lower_literal path source interpreter =
  let ranges = line_ranges source in
  let range =
    match interpreter with
    | "cmd" ->
        ranges
        |> List.find (fun (first, last) ->
            let line = String.sub source first (last - first) in
            String.lowercase_ascii line <> "@echo off")
    | _ -> List.hd ranges
  in
  let start_byte, end_byte = range in
  let line = String.sub source start_byte (end_byte - start_byte) in
  let argv =
    match interpreter with
    | "fish" ->
        let values = tokenize line in
        if values = [ [ Literal "command" ] ] then fail "missing fish command";
        List.tl values
    | "powershell" ->
        let values = tokenize line in
        if List.hd values <> [ Literal "&" ] then fail "missing call operator";
        List.tl values
    | "cmd" ->
        rewrite_first_literal
          (fun value -> String.sub value 1 (String.length value - 1))
          (tokenize line)
    | "nu" ->
        rewrite_first_literal
          (fun value -> String.sub value 1 (String.length value - 1))
          (tokenize line)
    | _ -> tokenize line
  in
  let basis =
    match interpreter with
    | "fish" -> "fish-static-external-command-v1"
    | "powershell" -> "powershell-static-external-command-v1"
    | "cmd" -> "cmd-static-external-command-v1"
    | "nu" -> "nu-static-external-command-v1"
    | _ -> fail "unsupported literal reference frontend: %s" interpreter
  in
  {
    body =
      {
        operation = Exec argv;
        guarantee = Native basis;
        source = Some (source_span path source start_byte end_byte);
      };
    environment = variables argv;
  }

let interpreter path =
  match String.lowercase_ascii (Filename.extension path) with
  | ".sh" -> "sh"
  | ".bash" -> "bash"
  | ".zsh" -> "zsh"
  | ".fish" -> "fish"
  | ".ps1" | ".psm1" -> "powershell"
  | ".cmd" | ".bat" -> "cmd"
  | ".nu" -> "nu"
  | _ -> "unknown"

let residual path source source_span_value interpreter reason =
  {
    body =
      {
        operation = Opaque_capsule { interpreter; source; path };
        guarantee = Residual reason;
        source = source_span_value;
      };
    environment = [];
  }

let default_interpreter_pin interpreter =
  "sha256:" ^ Sha256.hex ("deshell-official-runtime-v1:" ^ interpreter)

let delegated path source source_span_value interpreter reason =
  {
    body =
      {
        operation =
          Interpreter_call
            {
              interpreter;
              interpreter_pin = default_interpreter_pin interpreter;
              source;
              source_span = source_span_value;
              capabilities = [ "process"; "project_read"; "sandbox_write" ];
              reason;
            };
        guarantee = Delegated reason;
        source = Some source_span_value;
      };
    environment = [];
  }

let decoded_base64_length value =
  let length = String.length value in
  if length = 0 || length mod 4 <> 0 then fail "invalid canonical base64";
  let padding =
    if String.ends_with ~suffix:"==" value then 2
    else if String.ends_with ~suffix:"=" value then 1
    else 0
  in
  (length / 4 * 3) - padding

let lower golden =
  match (golden.source_utf8, golden.source_base64) with
  | Some source, None ->
      let interpreter = interpreter golden.path in
      if interpreter = "unknown" then
        residual golden.path (Utf8 source)
          (Some (source_span golden.path source 0 (String.length source)))
          interpreter
          "unknown frontend is trace-only; unobserved behavior is not claimed \
           as verified"
      else if interpreter = "sh" || interpreter = "bash" || interpreter = "zsh"
      then lower_posix golden.path source interpreter
      else lower_literal golden.path source interpreter
  | None, Some base64 ->
      let interpreter = interpreter golden.path in
      let bytes = decoded_base64_length base64 in
      let span =
        {
          file = golden.path;
          start_line = 1;
          start_column = 0;
          end_line = 1;
          end_column = bytes;
          start_byte = 0;
          end_byte = bytes;
        }
      in
      delegated golden.path (Base64 base64) span interpreter
        "source is not valid UTF-8 and cannot be statically lowered"
  | _ -> fail "%s must declare exactly one source encoding" golden.name

let rec counts node =
  let own =
    match node.guarantee with
    | Native _ -> (1, 0, 0)
    | Delegated _ -> (0, 1, 0)
    | Residual _ -> (0, 0, 1)
  in
  match node.operation with
  | Sequence nodes ->
      List.fold_left
        (fun (native, delegated, residual) node ->
          let next_native, next_delegated, next_residual = counts node in
          ( native + next_native,
            delegated + next_delegated,
            residual + next_residual ))
        own nodes
  | Exec _ | Set_variable _ | Interpreter_call _ | Opaque_capsule _ -> own

let string_member name value =
  match Yojson.Safe.Util.member name value with
  | `String result -> result
  | _ -> fail "%s must be a string" name

let int_member name value =
  match Yojson.Safe.Util.member name value with
  | `Int result -> result
  | _ -> fail "%s must be an integer" name

let optional_string_member name value =
  match Yojson.Safe.Util.member name value with
  | `String result -> Some result
  | `Null -> None
  | _ -> fail "%s must be a string or null" name

let parse_case value =
  {
    name = string_member "name" value;
    path = string_member "path" value;
    source_utf8 = optional_string_member "source_utf8" value;
    source_base64 = optional_string_member "source_base64" value;
    root_operation = string_member "root_operation" value;
    native = int_member "native" value;
    delegated = int_member "delegated" value;
    residual = int_member "residual" value;
    plan_digest = string_member "plan_digest" value;
  }

let load_corpus path =
  let value = Yojson.Safe.from_string (read_file path) in
  if int_member "schema_version" value <> 1 then
    fail "golden schema_version must be 1";
  match Yojson.Safe.Util.member "cases" value with
  | `List cases -> List.map parse_case cases
  | _ -> fail "golden cases must be an array"

let check_case golden =
  let lowered = lower golden in
  let actual_operation = operation_name lowered.body.operation in
  let native, delegated, residual = counts lowered.body in
  let digest = Sha256.hex (canonical_string (json_plan lowered)) in
  let differences = ref [] in
  let expect label expected actual =
    if expected <> actual then
      differences :=
        Printf.sprintf "%s expected %s, found %s" label expected actual
        :: !differences
  in
  expect "root_operation" golden.root_operation actual_operation;
  expect "native" (string_of_int golden.native) (string_of_int native);
  expect "delegated" (string_of_int golden.delegated) (string_of_int delegated);
  expect "residual" (string_of_int golden.residual) (string_of_int residual);
  expect "plan_digest" golden.plan_digest digest;
  match List.rev !differences with
  | [] -> None
  | values -> Some (golden.name ^ ": " ^ String.concat "; " values)

let find_backtick source start =
  let escaped = ref false in
  let index = ref start in
  let found = ref None in
  while !index < String.length source && !found = None do
    let current = source.[!index] in
    if !escaped then escaped := false
    else if current = '\\' then escaped := true
    else if current = '`' then found := Some !index;
    incr index
  done;
  !found

let safe_substitution body =
  String.trim body <> ""
  && not
       (String.exists
          (function
            | '\\' | '`' | '$' | '\n' | '\r' | '(' | ')' -> true | _ -> false)
          body)

let equivalent_rewrite source =
  let output = Buffer.create (String.length source + 16) in
  let state = ref None in
  let edits = ref 0 in
  let index = ref 0 in
  while !index < String.length source do
    let current = source.[!index] in
    match (!state, current) with
    | Some '\'', '\'' ->
        Buffer.add_char output current;
        state := None;
        incr index
    | Some '\'', _ ->
        Buffer.add_char output current;
        incr index
    | (None | Some '"'), '\\' ->
        Buffer.add_char output current;
        incr index;
        if !index < String.length source then begin
          Buffer.add_char output source.[!index];
          incr index
        end
    | None, '\'' ->
        Buffer.add_char output current;
        state := Some '\'';
        incr index
    | None, '"' ->
        Buffer.add_char output current;
        state := Some '"';
        incr index
    | Some '"', '"' ->
        Buffer.add_char output current;
        state := None;
        incr index
    | (None | Some '"'), '`' ->
        begin match find_backtick source (!index + 1) with
        | None ->
            Buffer.add_substring output source !index
              (String.length source - !index);
            index := String.length source
        | Some closing ->
            let body = String.sub source (!index + 1) (closing - !index - 1) in
            if safe_substitution body then begin
              Buffer.add_string output ("$(" ^ body ^ ")");
              incr edits
            end
            else Buffer.add_substring output source !index (closing - !index + 1);
            index := closing + 1
        end
    | _ ->
        Buffer.add_char output current;
        incr index
  done;
  (Buffer.contents output, !edits)

let has_strict_mode source =
  source |> String.split_on_char '\n'
  |> List.exists (fun line ->
      let line = String.trim line in
      String.starts_with ~prefix:"set -e" line
      || String.starts_with ~prefix:"set -o errexit" line)

let modernize source profiles =
  if List.mem "secure" profiles && not (has_strict_mode source) then
    let offset =
      if String.starts_with ~prefix:"#!" source then
        match String.index_opt source '\n' with
        | Some index -> index + 1
        | None -> String.length source
      else 0
    in
    ( String.sub source 0 offset ^ "set -eu\n"
      ^ String.sub source offset (String.length source - offset),
      1 )
  else (source, 0)

let rec literal_commands node =
  match node.operation with
  | Exec argv ->
      [
        List.map
          (function
            | [ Literal value ] -> value
            | _ ->
                fail "strict reference exporter received a dynamic expression")
          argv;
      ]
  | Sequence nodes -> List.concat_map literal_commands nodes
  | Set_variable _ | Interpreter_call _ | Opaque_capsule _ ->
      fail "strict reference exporter received %s"
        (operation_name node.operation)

let json_string value = Yojson.Safe.to_string (`String value)

let export_artifact target lowered =
  if lowered.environment <> [] then
    fail "strict reference exporter received an environment interface";
  let commands = literal_commands lowered.body in
  match (target, commands) with
  | "cwl", [ executable :: arguments ] ->
      let value =
        `Assoc
          [
            ( "arguments",
              `List (List.map (fun value -> `String value) arguments) );
            ("baseCommand", `List [ `String executable ]);
            ("class", `String "CommandLineTool");
            ("cwlVersion", `String "v1.2");
            ("inputs", `Assoc []);
            ( "outputs",
              `Assoc [ ("stdout", `Assoc [ ("type", `String "stdout") ]) ] );
            ("stdout", `String "deshell.stdout");
          ]
      in
      ("deshell.cwl", "application/cwl+json", pretty_string value)
  | "dagger", commands ->
      let steps =
        commands
        |> List.map (fun argv ->
            let encoded =
              `List (List.map (fun value -> `String value) argv)
              |> Yojson.Safe.to_string
            in
            "    container = container.withExec(" ^ encoded
            ^ ");\n    output += await container.stdout();")
        |> String.concat "\n"
      in
      let content =
        "import { dag, Container, object, func } from \"@dagger.io/dagger\";\n\n"
        ^ "@object()\nexport class Deshell {\n"
        ^ "  @func()\n  async main(): Promise<string> {\n"
        ^ "    let container: Container = \
           dag.container().from(\"ghcr.io/deshell-lang/lab@sha256:14358309a308569c32bdc37e2e0e9694be33a9d99e68afb0f5ff33cc1f695dce\");\n"
        ^ "    let output = \"\";\n" ^ steps ^ "\n    return output;\n  }\n}\n"
      in
      ("deshell.dagger.ts", "text/typescript", content)
  | "nu", commands ->
      let lines =
        commands
        |> List.map (fun argv ->
            "  run-external " ^ String.concat " " (List.map json_string argv))
        |> String.concat "\n"
      in
      ( "deshell.nu",
        "text/x-nushell",
        "export def main [] {\n" ^ lines ^ "\n}\n" )
  | "cwl", _ -> fail "strict CWL reference export requires exactly one command"
  | target, _ -> fail "unknown reference export target: %s" target

let list_member name value =
  match Yojson.Safe.Util.member name value with
  | `List values -> values
  | _ -> fail "%s must be an array" name

let string_list_member name value =
  list_member name value
  |> List.map (function
    | `String item -> item
    | _ -> fail "%s entries must be strings" name)

let check_transform_corpus path =
  let value = Yojson.Safe.from_string (read_file path) in
  if int_member "schema_version" value <> 1 then
    fail "transform schema_version must be 1";
  let failures = ref [] in
  let expect case label expected actual =
    if expected <> actual then
      failures :=
        Printf.sprintf "%s: %s expected %s, found %s" case label expected actual
        :: !failures
  in
  list_member "equivalent_rewrites" value
  |> List.iter (fun case ->
      let name = string_member "name" case in
      let output, edits = equivalent_rewrite (string_member "source" case) in
      expect name "output" (string_member "output" case) output;
      expect name "edits"
        (string_of_int (int_member "edits" case))
        (string_of_int edits));
  list_member "modernizations" value
  |> List.iter (fun case ->
      let name = string_member "name" case in
      let output, edits =
        modernize
          (string_member "source" case)
          (string_list_member "profiles" case)
      in
      expect name "output" (string_member "output" case) output;
      expect name "edits"
        (string_of_int (int_member "edits" case))
        (string_of_int edits));
  list_member "exports" value
  |> List.iter (fun case ->
      let name = string_member "name" case in
      let source = string_member "source" case in
      let golden =
        {
          name;
          path = string_member "path" case;
          source_utf8 = Some source;
          source_base64 = None;
          root_operation = "";
          native = 0;
          delegated = 0;
          residual = 0;
          plan_digest = "";
        }
      in
      let filename, media_type, content =
        export_artifact (string_member "target" case) (lower golden)
      in
      expect name "filename" (string_member "filename" case) filename;
      expect name "media_type" (string_member "media_type" case) media_type;
      expect name "content_sha256"
        (string_member "content_sha256" case)
        (Sha256.hex content));
  List.rev !failures

let run arguments =
  let path =
    if Array.length arguments >= 2 then arguments.(1) else default_corpus
  in
  let transform_path =
    if Array.length arguments >= 3 then arguments.(2)
    else default_transform_corpus
  in
  try
    let cases = load_corpus path in
    let failures =
      List.filter_map check_case cases @ check_transform_corpus transform_path
    in
    if failures = [] then begin
      Printf.printf
        "OCaml Effect IR v1 reference matched %d frontend and transform/export \
         shared cases\n"
        (List.length cases);
      0
    end
    else begin
      List.iter prerr_endline failures;
      1
    end
  with
  | Failure message ->
      prerr_endline ("reference-v1: " ^ message);
      1
  | Yojson.Json_error message ->
      prerr_endline ("reference-v1: invalid frontend-v1.json: " ^ message);
      1
