let current_version = 2

type t = {
  version : int;
  migrated_from : int option;
  adapter_protocol : int;
  effect_ir : int;
  command_model_digest : string;
  lab_image : string;
  interpreters : (string * string) list;
  adapters : (string * string) list;
}

type value = String of string | Integer of int

let ( let* ) result continuation =
  match result with Ok value -> continuation value | Error _ as error -> error

let is_hex = function '0' .. '9' | 'a' .. 'f' -> true | _ -> false

let digest value =
  if
    String.length value = 71
    && String.starts_with ~prefix:"sha256:" value
    && String.sub value 7 64 |> String.for_all is_hex
  then Ok (String.sub value 7 64)
  else Error "must be a lowercase sha256:<64 hex characters> digest"

let strip_comment line =
  let rec loop index quoted escaped =
    if index = String.length line then line
    else
      match line.[index] with
      | '#' when not quoted -> String.sub line 0 index
      | '\\' when quoted -> loop (index + 1) quoted (not escaped)
      | '"' when not escaped -> loop (index + 1) (not quoted) false
      | _ -> loop (index + 1) quoted false
  in
  loop 0 false false

let parse_value text =
  let text = String.trim text in
  if text = "" then Error "missing value"
  else if text.[0] = '"' then
    try
      match Yojson.Safe.from_string text with
      | `String value -> Ok (String value)
      | _ -> Error "expected a TOML string"
    with Yojson.Json_error message -> Error ("invalid string: " ^ message)
  else
    try Ok (Integer (int_of_string text))
    with Failure _ -> Error "expected a quoted string or integer"

let parse source =
  let section = ref "" in
  let sections = Hashtbl.create 16 in
  let values = Hashtbl.create 32 in
  let errors = ref [] in
  let error line message =
    errors := Printf.sprintf "deshell.lock:%d: %s" line message :: !errors
  in
  source |> String.split_on_char '\n'
  |> List.iteri (fun index raw ->
      let line_number = index + 1 in
      let line = strip_comment raw |> String.trim in
      if line = "" then ()
      else if line.[0] = '[' then
        if String.length line < 3 || line.[String.length line - 1] <> ']' then
          error line_number "malformed table header"
        else
          let name =
            String.sub line 1 (String.length line - 2) |> String.trim
          in
          if name = "" then error line_number "empty table name"
          else if Hashtbl.mem sections name then
            error line_number ("duplicate table: " ^ name)
          else begin
            Hashtbl.add sections name ();
            section := name
          end
      else
        match String.index_opt line '=' with
        | None -> error line_number "expected key = value"
        | Some separator -> (
            let key = String.sub line 0 separator |> String.trim in
            let raw_value =
              String.sub line (separator + 1)
                (String.length line - separator - 1)
            in
            if key = "" then error line_number "empty key"
            else
              let qualified =
                if !section = "" then key else !section ^ "." ^ key
              in
              if Hashtbl.mem values qualified then
                error line_number ("duplicate key: " ^ qualified)
              else
                match parse_value raw_value with
                | Ok value -> Hashtbl.add values qualified value
                | Error message -> error line_number (qualified ^ ": " ^ message)
            ));
  if !errors = [] then Ok values else Error (List.rev !errors)

let decode_string source =
  let* values = parse source in
  let errors = ref [] in
  let layout_version =
    match Hashtbl.find_opt values "version" with
    | Some (Integer ((1 | 2) as version)) -> version
    | Some (Integer version) ->
        errors :=
          Printf.sprintf "unsupported deshell.lock version: %d" version
          :: !errors;
        current_version
    | Some (String _) ->
        errors := "version must be an integer" :: !errors;
        current_version
    | None ->
        errors := "deshell.lock is missing required key: version" :: !errors;
        current_version
  in
  let migrated_from =
    if layout_version = 1 then begin
      let add_if_missing name value =
        if not (Hashtbl.mem values name) then Hashtbl.add values name value
      in
      add_if_missing "artifacts.command_model"
        (String ("sha256:" ^ Command_model.digest ()));
      add_if_missing "adapters.powershell"
        (String
           "sha256:1129a13d31daa18e248e5f31f90527ed54daf98445b99802a6cf2602f44af66f");
      add_if_missing "adapters.nushell"
        (String
           "sha256:9ff57da1e3b91d67b616648692ac2aff86bb8623e9ca3ad65ab9f949dfbe1b5a");
      add_if_missing "adapters.nushell_dependencies"
        (String
           "sha256:e8c9df018d2570fa8153c50c68ad459cbecc1fd2c120919f91726c87ba42107e");
      List.iter
        (fun name ->
          add_if_missing ("interpreters." ^ name)
            (String "provided-by-lab-image"))
        [ "posix_sh"; "bash"; "zsh"; "fish"; "powershell"; "cmd"; "nushell" ];
      add_if_missing "lab.image" (String "unconfigured");
      Hashtbl.replace values "protocol.effect_ir"
        (Integer Ir.current_schema_version);
      Some 1
    end
    else None
  in
  let required name =
    match Hashtbl.find_opt values name with
    | Some value -> Some value
    | None ->
        errors := ("deshell.lock is missing required key: " ^ name) :: !errors;
        None
  in
  let integer name expected =
    match required name with
    | Some (Integer value) when value = expected -> value
    | Some (Integer value) ->
        errors :=
          Printf.sprintf "%s must be %d (found %d)" name expected value
          :: !errors;
        value
    | Some (String _) ->
        errors := (name ^ " must be an integer") :: !errors;
        expected
    | None -> expected
  in
  let string name =
    match required name with
    | Some (String value) -> value
    | Some (Integer _) ->
        errors := (name ^ " must be a string") :: !errors;
        ""
    | None -> ""
  in
  let version = current_version in
  let adapter_protocol = integer "protocol.adapter" Plugin_protocol.version in
  let effect_ir = integer "protocol.effect_ir" Ir.current_schema_version in
  let command_model = string "artifacts.command_model" in
  let command_model_digest =
    match digest command_model with
    | Ok value -> value
    | Error message ->
        errors := ("artifacts.command_model " ^ message) :: !errors;
        ""
  in
  if
    command_model_digest <> ""
    && command_model_digest <> Command_model.digest ()
  then
    errors :=
      "command model digest does not match this deshell binary" :: !errors;
  let adapter_names = [ "powershell"; "nushell"; "nushell_dependencies" ] in
  let adapters =
    List.map
      (fun name ->
        let value = string ("adapters." ^ name) in
        begin match digest value with
        | Ok _ -> ()
        | Error message ->
            errors := ("adapters." ^ name ^ " " ^ message) :: !errors
        end;
        (name, value))
      adapter_names
  in
  let interpreter_names =
    [ "posix_sh"; "bash"; "zsh"; "fish"; "powershell"; "cmd"; "nushell" ]
  in
  let interpreters =
    List.map
      (fun name ->
        let value = string ("interpreters." ^ name) in
        if value = "" then
          errors := ("interpreters." ^ name ^ " must not be empty") :: !errors;
        (name, value))
      interpreter_names
  in
  let lab_image = string "lab.image" in
  if !errors <> [] then Error (List.rev !errors)
  else
    Ok
      {
        version;
        migrated_from;
        adapter_protocol;
        effect_ir;
        command_model_digest;
        lab_image;
        interpreters;
        adapters;
      }

let default () =
  Printf.sprintf
    {|version = 2

[toolchain]
ocaml = "5.5.0"
dune = "3.24"
opam = "2.5.2"

[protocol]
adapter = 1
effect_ir = %d

[artifacts]
command_model = "sha256:%s"

[adapters]
powershell = "sha256:1129a13d31daa18e248e5f31f90527ed54daf98445b99802a6cf2602f44af66f"
nushell = "sha256:9ff57da1e3b91d67b616648692ac2aff86bb8623e9ca3ad65ab9f949dfbe1b5a"
nushell_dependencies = "sha256:e8c9df018d2570fa8153c50c68ad459cbecc1fd2c120919f91726c87ba42107e"

[interpreters]
posix_sh = "provided-by-lab-image"
bash = "provided-by-lab-image"
zsh = "provided-by-lab-image"
fish = "provided-by-lab-image"
powershell = "provided-by-lab-image"
cmd = "provided-by-lab-image"
nushell = "provided-by-lab-image"

[lab]
image = "unconfigured"
|}
    Ir.current_schema_version (Command_model.digest ())

let observation_image lock =
  if lock.lab_image = "unconfigured" then
    Error "deshell.lock lab.image is unconfigured"
  else if Lab.digest_pinned lock.lab_image then Ok lock.lab_image
  else Error "deshell.lock lab.image must be pinned by sha256 digest"

let load ~root =
  let path = Filename.concat root "deshell.lock" in
  try
    let channel = open_in_bin path in
    let source =
      Fun.protect
        ~finally:(fun () -> close_in_noerr channel)
        (fun () -> really_input_string channel (in_channel_length channel))
    in
    decode_string source
  with Sys_error message -> Error [ message ]
