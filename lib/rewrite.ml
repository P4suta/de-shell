type edit = { rule : string; original : Ir.source_span; replacement : string }
type result = { output : string; edits : edit list }

let position_at source offset =
  let line = ref 1 in
  let column = ref 0 in
  for index = 0 to offset - 1 do
    if source.[index] = '\n' then begin
      incr line;
      column := 0
    end
    else incr column
  done;
  (!line, !column)

let span ~path source start_byte end_byte =
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

let safe_substitution body =
  String.trim body <> ""
  && String.for_all
       (function
         | '\\' | '`' | '$' | '\n' | '\r' | '(' | ')' -> false | _ -> true)
       body

let find_closing source start =
  let index = ref start in
  let escaped = ref false in
  let found = ref None in
  while !index < String.length source && !found = None do
    let character = source.[!index] in
    if !escaped then escaped := false
    else if character = '\\' then escaped := true
    else if character = '`' then found := Some !index;
    incr index
  done;
  !found

let equivalent ~path source =
  let output = Buffer.create (String.length source + 16) in
  let edits = ref [] in
  let state = ref `Normal in
  let index = ref 0 in
  while !index < String.length source do
    let character = source.[!index] in
    match (!state, character) with
    | `Single, '\'' ->
        Buffer.add_char output character;
        state := `Normal;
        incr index
    | `Single, _ ->
        Buffer.add_char output character;
        incr index
    | (`Normal | `Double), '\\' ->
        Buffer.add_char output character;
        incr index;
        if !index < String.length source then begin
          Buffer.add_char output source.[!index];
          incr index
        end
    | `Normal, '\'' ->
        Buffer.add_char output character;
        state := `Single;
        incr index
    | `Normal, '"' ->
        Buffer.add_char output character;
        state := `Double;
        incr index
    | `Double, '"' ->
        Buffer.add_char output character;
        state := `Normal;
        incr index
    | (`Normal | `Double), '`' ->
        begin match find_closing source (!index + 1) with
        | Some closing ->
            let body = String.sub source (!index + 1) (closing - !index - 1) in
            if safe_substitution body then begin
              let replacement = "$(" ^ body ^ ")" in
              Buffer.add_string output replacement;
              edits :=
                {
                  rule = "posix.backticks.simple";
                  original = span ~path source !index (closing + 1);
                  replacement;
                }
                :: !edits;
              index := closing + 1
            end
            else begin
              Buffer.add_substring output source !index (closing - !index + 1);
              index := closing + 1
            end
        | None ->
            Buffer.add_substring output source !index
              (String.length source - !index);
            index := String.length source
        end
    | _, _ ->
        Buffer.add_char output character;
        incr index
  done;
  { output = Buffer.contents output; edits = List.rev !edits }
