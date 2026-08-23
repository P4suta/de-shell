type host_write = Deny | Project
type network = Deny_network | Record_replay
type unknown_interpreter = Trace_only | Reject
type sandbox_mode = Disposable

type policy = {
  host_write : host_write;
  network : network;
  unknown_interpreter : unknown_interpreter;
}

type export = { strict : bool; bridge : bool }

type t = {
  version : int;
  entrypoints : string list;
  policy : policy;
  sandbox_mode : sandbox_mode;
  export : export;
}

type section = Root | Policy | Sandbox | Export | Invalid

let strip_comment line =
  let rec loop index quoted escaped =
    if index = String.length line then line
    else
      match (line.[index], quoted, escaped) with
      | '#', false, _ -> String.sub line 0 index
      | '\\', true, false -> loop (index + 1) true true
      | '"', _, false -> loop (index + 1) (not quoted) false
      | _, _, _ -> loop (index + 1) quoted false
  in
  loop 0 false false

let split_assignment line =
  match String.index_opt line '=' with
  | None -> None
  | Some index ->
      Some
        ( String.sub line 0 index |> String.trim,
          String.sub line (index + 1) (String.length line - index - 1)
          |> String.trim )

let decode_string_value value =
  try
    match Yojson.Safe.from_string value with
    | `String value -> Ok value
    | _ -> Error "expected a quoted string"
  with Yojson.Json_error message -> Error ("invalid string: " ^ message)

let decode_string_list value =
  try
    match Yojson.Safe.from_string value with
    | `List values ->
        let rec loop accumulator = function
          | [] -> Ok (List.rev accumulator)
          | `String value :: rest -> loop (value :: accumulator) rest
          | _ -> Error "expected an array of strings"
        in
        loop [] values
    | _ -> Error "expected an array of strings"
  with Yojson.Json_error message -> Error ("invalid array: " ^ message)

let safe_entrypoint path =
  let normalized =
    String.map
      (fun character -> if character = '\\' then '/' else character)
      path
  in
  path <> "" && Filename.is_relative path
  && String.for_all (function '\000' | ':' -> false | _ -> true) path
  && normalized |> String.split_on_char '/'
     |> List.for_all (fun part -> part <> "" && part <> "." && part <> "..")

let decode_string source =
  let errors = ref [] in
  let add_error line message =
    errors := Printf.sprintf "project.toml:%d: %s" line message :: !errors
  in
  let section = ref Root in
  let sections = Hashtbl.create 4 in
  let fields = Hashtbl.create 16 in
  let version = ref None in
  let entrypoints = ref None in
  let host_write = ref None in
  let network = ref None in
  let unknown_interpreter = ref None in
  let sandbox_mode = ref None in
  let strict = ref None in
  let bridge = ref None in
  let mark line name =
    if Hashtbl.mem fields name then begin
      add_error line ("duplicate field: " ^ name);
      false
    end
    else begin
      Hashtbl.add fields name ();
      true
    end
  in
  let set line name decoder value assign =
    if mark line name then
      match decoder value with
      | Ok value -> assign value
      | Error message -> add_error line (name ^ ": " ^ message)
  in
  source |> String.split_on_char '\n'
  |> List.iteri (fun index raw ->
      let line_number = index + 1 in
      let line = strip_comment raw |> String.trim in
      if line = "" then ()
      else if line.[0] = '[' then
        let name =
          if String.length line >= 3 && line.[String.length line - 1] = ']' then
            String.sub line 1 (String.length line - 2) |> String.trim
          else ""
        in
        if name = "" then begin
          add_error line_number "malformed table header";
          section := Invalid
        end
        else if Hashtbl.mem sections name then begin
          add_error line_number ("duplicate table: " ^ name);
          section := Invalid
        end
        else begin
          Hashtbl.add sections name ();
          section :=
            match name with
            | "policy" -> Policy
            | "sandbox" -> Sandbox
            | "export" -> Export
            | _ ->
                add_error line_number ("unknown table: " ^ name);
                Invalid
        end
      else
        match split_assignment line with
        | None -> add_error line_number "expected key = value"
        | Some (key, value) ->
            begin match (!section, key) with
            | Root, "version" ->
                set line_number "version"
                  (fun value ->
                    try Ok (int_of_string value)
                    with Failure _ -> Error "expected an integer")
                  value
                  (fun value -> version := Some value)
            | Root, "entrypoints" ->
                set line_number "entrypoints" decode_string_list value
                  (fun value -> entrypoints := Some value)
            | Policy, "host_write" ->
                set line_number "policy.host_write" decode_string_value value
                  (fun value -> host_write := Some value)
            | Policy, "network" ->
                set line_number "policy.network" decode_string_value value
                  (fun value -> network := Some value)
            | Policy, "unknown_interpreter" ->
                set line_number "policy.unknown_interpreter" decode_string_value
                  value (fun value -> unknown_interpreter := Some value)
            | Sandbox, "mode" ->
                set line_number "sandbox.mode" decode_string_value value
                  (fun value -> sandbox_mode := Some value)
            | Export, "strict" ->
                set line_number "export.strict"
                  (function
                    | "true" -> Ok true
                    | "false" -> Ok false
                    | _ -> Error "expected true or false")
                  value
                  (fun value -> strict := Some value)
            | Export, "bridge" ->
                set line_number "export.bridge"
                  (function
                    | "true" -> Ok true
                    | "false" -> Ok false
                    | _ -> Error "expected true or false")
                  value
                  (fun value -> bridge := Some value)
            | Invalid, _ ->
                add_error line_number "field belongs to an invalid table"
            | _, _ -> add_error line_number ("unknown field: " ^ key)
            end);
  let required name value default =
    match value with
    | Some value -> value
    | None ->
        errors := ("project.toml is missing required field: " ^ name) :: !errors;
        default
  in
  let version = required "version" !version 1 in
  if version <> 1 then
    errors :=
      Printf.sprintf "project.toml version must be 1 (found %d)" version
      :: !errors;
  let entrypoints = required "entrypoints" !entrypoints [] in
  if
    List.length entrypoints
    <> List.length (List.sort_uniq String.compare entrypoints)
  then
    errors := "project.toml entrypoints must not contain duplicates" :: !errors;
  List.iter
    (fun path ->
      if not (safe_entrypoint path) then
        errors := ("unsafe project entrypoint: " ^ path) :: !errors)
    entrypoints;
  let host_write =
    match required "policy.host_write" !host_write "deny" with
    | "deny" -> Deny
    | "project" -> Project
    | value ->
        errors := ("invalid policy.host_write: " ^ value) :: !errors;
        Deny
  in
  let network =
    match required "policy.network" !network "deny" with
    | "deny" -> Deny_network
    | "record-replay" -> Record_replay
    | value ->
        errors := ("invalid policy.network: " ^ value) :: !errors;
        Deny_network
  in
  let unknown_interpreter =
    match
      required "policy.unknown_interpreter" !unknown_interpreter "trace-only"
    with
    | "trace-only" -> Trace_only
    | "reject" -> Reject
    | value ->
        errors := ("invalid policy.unknown_interpreter: " ^ value) :: !errors;
        Trace_only
  in
  let sandbox_mode =
    match required "sandbox.mode" !sandbox_mode "disposable" with
    | "disposable" -> Disposable
    | value ->
        errors := ("invalid sandbox.mode: " ^ value) :: !errors;
        Disposable
  in
  let strict = required "export.strict" !strict true in
  let bridge = required "export.bridge" !bridge false in
  if (not strict) && not bridge then
    errors :=
      "export.strict=false requires export.bridge=true to avoid silent loss"
      :: !errors;
  if !errors <> [] then Error (List.rev !errors)
  else
    Ok
      {
        version;
        entrypoints;
        policy = { host_write; network; unknown_interpreter };
        sandbox_mode;
        export = { strict; bridge };
      }

let load ~root =
  let path = Filename.concat (Filename.concat root ".deshell") "project.toml" in
  try
    let channel = open_in_bin path in
    let source =
      Fun.protect
        ~finally:(fun () -> close_in_noerr channel)
        (fun () -> really_input_string channel (in_channel_length channel))
    in
    decode_string source
  with Sys_error message -> Error [ message ]
