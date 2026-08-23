let alphabet =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"

let encode source =
  let output = Buffer.create ((String.length source + 2) / 3 * 4) in
  let byte index = Char.code source.[index] in
  let rec loop index =
    if index >= String.length source then Buffer.contents output
    else begin
      let remaining = String.length source - index in
      let first = byte index in
      let second = if remaining > 1 then byte (index + 1) else 0 in
      let third = if remaining > 2 then byte (index + 2) else 0 in
      let value = (first lsl 16) lor (second lsl 8) lor third in
      Buffer.add_char output alphabet.[(value lsr 18) land 0x3f];
      Buffer.add_char output alphabet.[(value lsr 12) land 0x3f];
      Buffer.add_char output
        (if remaining > 1 then alphabet.[(value lsr 6) land 0x3f] else '=');
      Buffer.add_char output
        (if remaining > 2 then alphabet.[value land 0x3f] else '=');
      loop (index + 3)
    end
  in
  loop 0

let decode_character = function
  | 'A' .. 'Z' as value -> Some (Char.code value - Char.code 'A')
  | 'a' .. 'z' as value -> Some (26 + Char.code value - Char.code 'a')
  | '0' .. '9' as value -> Some (52 + Char.code value - Char.code '0')
  | '+' -> Some 62
  | '/' -> Some 63
  | _ -> None

let decode source =
  if String.length source mod 4 <> 0 then
    Error "base64 length must be a multiple of four"
  else
    let output = Buffer.create (String.length source / 4 * 3) in
    let error = ref None in
    let index = ref 0 in
    while !index < String.length source && !error = None do
      let final = !index + 4 = String.length source in
      let a = source.[!index] in
      let b = source.[!index + 1] in
      let c = source.[!index + 2] in
      let d = source.[!index + 3] in
      begin match (decode_character a, decode_character b) with
      | Some first, Some second ->
          begin match (c, d, decode_character c, decode_character d) with
          | '=', '=', _, _ when final && second land 0x0f = 0 ->
              Buffer.add_char output
                (Char.chr ((first lsl 2) lor (second lsr 4)))
          | _, '=', Some third, _ when final && third land 0x03 = 0 ->
              Buffer.add_char output
                (Char.chr ((first lsl 2) lor (second lsr 4)));
              Buffer.add_char output
                (Char.chr (((second land 0x0f) lsl 4) lor (third lsr 2)))
          | _, _, Some third, Some fourth ->
              Buffer.add_char output
                (Char.chr ((first lsl 2) lor (second lsr 4)));
              Buffer.add_char output
                (Char.chr (((second land 0x0f) lsl 4) lor (third lsr 2)));
              Buffer.add_char output
                (Char.chr (((third land 0x03) lsl 6) lor fourth))
          | _ -> error := Some "invalid base64 padding or character"
          end
      | _ -> error := Some "invalid base64 character"
      end;
      index := !index + 4
    done;
    match !error with
    | Some message -> Error message
    | None -> Ok (Buffer.contents output)
