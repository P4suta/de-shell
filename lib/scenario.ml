type fixture = { path : string; contents : string; executable : bool }
type expected_file = { path : string; sha256 : string }

type expectation = {
  exit_code : int option;
  stdout : string option;
  stderr : string option;
  files : expected_file list;
}

type t = {
  name : string;
  args : string list;
  environment : (string * string) list;
  fixtures : fixture list;
  timeout_ms : int;
  expect : expectation;
}

type fixture_builder = {
  index : int;
  mutable path : string option;
  mutable contents : string option;
  mutable executable : bool option;
}

type expected_file_builder = {
  index : int;
  mutable path : string option;
  mutable sha256 : string option;
}

type section =
  | Root
  | Environment
  | Fixture of fixture_builder
  | Expect
  | Expected_file of expected_file_builder
  | Invalid

let trim = String.trim

let strip_comment line =
  let length = String.length line in
  let rec loop index quote escaped =
    if index = length then line
    else
      let character = line.[index] in
      match quote with
      | Some '"' when escaped -> loop (index + 1) quote false
      | Some '"' when character = '\\' -> loop (index + 1) quote true
      | Some delimiter when character = delimiter -> loop (index + 1) None false
      | Some _ -> loop (index + 1) quote false
      | None when character = '"' || character = '\'' ->
          loop (index + 1) (Some character) false
      | None when character = '#' -> String.sub line 0 index
      | None -> loop (index + 1) None false
  in
  loop 0 None false

let split_assignment line =
  let length = String.length line in
  let rec loop index quote escaped =
    if index = length then None
    else
      let character = line.[index] in
      match quote with
      | Some '"' when escaped -> loop (index + 1) quote false
      | Some '"' when character = '\\' -> loop (index + 1) quote true
      | Some delimiter when character = delimiter -> loop (index + 1) None false
      | Some _ -> loop (index + 1) quote false
      | None when character = '"' || character = '\'' ->
          loop (index + 1) (Some character) false
      | None when character = '=' ->
          Some
            ( trim (String.sub line 0 index),
              trim (String.sub line (index + 1) (length - index - 1)) )
      | None -> loop (index + 1) None false
  in
  loop 0 None false

let decode_string_value value =
  let length = String.length value in
  if length >= 2 && value.[0] = '\'' && value.[length - 1] = '\'' then
    Ok (String.sub value 1 (length - 2))
  else
    try
      match Yojson.Safe.from_string value with
      | `String text -> Ok text
      | _ -> Error "expected a string"
    with Yojson.Json_error message -> Error ("invalid string: " ^ message)

let decode_string_list value =
  try
    match Yojson.Safe.from_string value with
    | `List values ->
        let rec collect accumulator = function
          | [] -> Ok (List.rev accumulator)
          | `String text :: rest -> collect (text :: accumulator) rest
          | _ -> Error "expected an array of strings"
        in
        collect [] values
    | _ -> Error "expected an array of strings"
  with Yojson.Json_error message -> Error ("invalid array: " ^ message)

let decode_int value =
  try Ok (int_of_string value) with Failure _ -> Error "expected an integer"

let decode_bool = function
  | "true" -> Ok true
  | "false" -> Ok false
  | _ -> Error "expected true or false"

let is_hex_digest value =
  String.length value = 64
  && String.for_all
       (function '0' .. '9' | 'a' .. 'f' | 'A' .. 'F' -> true | _ -> false)
       value

let safe_relative_path path =
  let normalized =
    String.map
      (fun character -> if character = '\\' then '/' else character)
      path
  in
  Filename.is_relative path && normalized <> ""
  && (not (String.starts_with ~prefix:"/" normalized))
  && normalized |> String.split_on_char '/'
     |> List.for_all (fun part -> part <> "" && part <> "." && part <> "..")

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

let decode_string source =
  let errors = ref [] in
  let add_error line message =
    errors := Printf.sprintf "line %d: %s" line message :: !errors
  in
  let seen = Hashtbl.create 32 in
  let mark line field =
    if Hashtbl.mem seen field then begin
      add_error line ("duplicate field: " ^ field);
      false
    end
    else begin
      Hashtbl.add seen field ();
      true
    end
  in
  let name = ref None in
  let args = ref [] in
  let timeout_ms = ref 30000 in
  let environment = ref [] in
  let fixtures = ref [] in
  let expected_files = ref [] in
  let exit_code = ref None in
  let stdout = ref None in
  let stderr = ref None in
  let fixture_count = ref 0 in
  let expected_file_count = ref 0 in
  let section = ref Root in
  let finish_section line =
    begin match !section with
    | Fixture builder ->
        begin match (builder.path, builder.contents) with
        | Some path, Some contents ->
            if safe_relative_path path then
              fixtures :=
                {
                  path;
                  contents;
                  executable = Option.value ~default:false builder.executable;
                }
                :: !fixtures
            else
              add_error line
                ("fixture path must be a safe project-relative path: " ^ path)
        | None, _ -> add_error line "fixture is missing path"
        | _, None -> add_error line "fixture is missing contents"
        end
    | Expected_file builder ->
        begin match (builder.path, builder.sha256) with
        | Some path, Some sha256 ->
            if not (safe_relative_path path) then
              add_error line
                ("expected file path must be a safe project-relative path: "
               ^ path)
            else if not (is_hex_digest sha256) then
              add_error line
                ("expected file sha256 must be a 64-character hex digest: "
               ^ path)
            else expected_files := { path; sha256 } :: !expected_files
        | None, _ -> add_error line "expected file is missing path"
        | _, None -> add_error line "expected file is missing sha256"
        end
    | Root | Environment | Expect | Invalid -> ()
    end;
    section := Invalid
  in
  let set_decoded line field decode value setter =
    if mark line field then
      match decode value with
      | Ok decoded -> setter decoded
      | Error message -> add_error line (field ^ " " ^ message)
  in
  source |> String.split_on_char '\n'
  |> List.iteri (fun index raw_line ->
      let line_number = index + 1 in
      let line = raw_line |> strip_comment |> trim in
      if line = "" then ()
      else if String.starts_with ~prefix:"[[" line then begin
        finish_section line_number;
        if line = "[[fixtures]]" then begin
          let index = !fixture_count in
          incr fixture_count;
          section :=
            Fixture { index; path = None; contents = None; executable = None }
        end
        else if line = "[[expect.files]]" then begin
          let index = !expected_file_count in
          incr expected_file_count;
          section := Expected_file { index; path = None; sha256 = None }
        end
        else begin
          add_error line_number ("unknown array table: " ^ line);
          section := Invalid
        end
      end
      else if String.starts_with ~prefix:"[" line then begin
        finish_section line_number;
        section :=
          match line with
          | "[environment]" -> Environment
          | "[expect]" -> Expect
          | _ ->
              add_error line_number ("unknown table: " ^ line);
              Invalid
      end
      else
        match split_assignment line with
        | None -> add_error line_number "expected key = value"
        | Some (key, value) ->
            begin match !section with
            | Root ->
                begin match key with
                | "name" ->
                    set_decoded line_number "name" decode_string_value value
                      (fun value -> name := Some value)
                | "args" ->
                    set_decoded line_number "args" decode_string_list value
                      (fun value -> args := value)
                | "timeout_ms" ->
                    set_decoded line_number "timeout_ms" decode_int value
                      (fun value ->
                        if value <= 0 then
                          add_error line_number
                            "timeout_ms must be greater than zero"
                        else timeout_ms := value)
                | _ -> add_error line_number ("unknown root field: " ^ key)
                end
            | Environment ->
                if not (valid_environment_name key) then
                  add_error line_number
                    ("invalid environment variable name: " ^ key)
                else
                  set_decoded line_number ("environment." ^ key)
                    decode_string_value value (fun value ->
                      environment := (key, value) :: !environment)
            | Fixture builder ->
                let prefix = Printf.sprintf "fixtures[%d]." builder.index in
                begin match key with
                | "path" ->
                    set_decoded line_number (prefix ^ key) decode_string_value
                      value (fun value -> builder.path <- Some value)
                | "contents" ->
                    set_decoded line_number (prefix ^ key) decode_string_value
                      value (fun value -> builder.contents <- Some value)
                | "executable" ->
                    set_decoded line_number (prefix ^ key) decode_bool value
                      (fun value -> builder.executable <- Some value)
                | _ -> add_error line_number ("unknown fixture field: " ^ key)
                end
            | Expect ->
                begin match key with
                | "exit_code" ->
                    set_decoded line_number "expect.exit_code" decode_int value
                      (fun value -> exit_code := Some value)
                | "stdout" ->
                    set_decoded line_number "expect.stdout" decode_string_value
                      value (fun value -> stdout := Some value)
                | "stderr" ->
                    set_decoded line_number "expect.stderr" decode_string_value
                      value (fun value -> stderr := Some value)
                | _ -> add_error line_number ("unknown expect field: " ^ key)
                end
            | Expected_file builder ->
                let prefix = Printf.sprintf "expect.files[%d]." builder.index in
                begin match key with
                | "path" ->
                    set_decoded line_number (prefix ^ key) decode_string_value
                      value (fun value -> builder.path <- Some value)
                | "sha256" ->
                    set_decoded line_number (prefix ^ key) decode_string_value
                      value (fun value -> builder.sha256 <- Some value)
                | _ ->
                    add_error line_number ("unknown expected file field: " ^ key)
                end
            | Invalid ->
                add_error line_number "field belongs to an invalid table"
            end);
  finish_section (List.length (String.split_on_char '\n' source) + 1);
  begin match !name with
  | None -> errors := "scenario is missing required field: name" :: !errors
  | Some "" -> errors := "scenario name must not be empty" :: !errors
  | Some _ -> ()
  end;
  if !errors <> [] then Error (List.rev !errors)
  else
    Ok
      {
        name = Option.get !name;
        args = !args;
        environment = List.sort compare !environment;
        fixtures = List.rev !fixtures;
        timeout_ms = !timeout_ms;
        expect =
          {
            exit_code = !exit_code;
            stdout = !stdout;
            stderr = !stderr;
            files = List.rev !expected_files;
          };
      }

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let load_directory path =
  try
    if not (Sys.file_exists path && Sys.is_directory path) then
      Error [ "scenario directory does not exist: " ^ path ]
    else
      let files =
        Sys.readdir path |> Array.to_list
        |> List.filter (String.ends_with ~suffix:".toml")
        |> List.sort String.compare
      in
      let rec load scenarios errors = function
        | [] ->
            if errors = [] then Ok (List.rev scenarios)
            else Error (List.rev errors |> List.flatten)
        | filename :: rest ->
            let full_path = Filename.concat path filename in
            begin match decode_string (read_file full_path) with
            | Ok scenario -> load (scenario :: scenarios) errors rest
            | Error messages ->
                let messages =
                  List.map (fun message -> filename ^ ": " ^ message) messages
                in
                load scenarios (messages :: errors) rest
            end
      in
      load [] [] files
  with Sys_error message -> Error [ message ]

let expectation_errors scenario (observation : Observation.t) =
  let errors = ref [] in
  let mismatch field expected actual =
    errors :=
      Printf.sprintf "%s %s mismatch (expected %s, observed %s)" scenario.name
        field expected actual
      :: !errors
  in
  Option.iter
    (fun expected ->
      if observation.exit_code <> expected then
        mismatch "expect.exit_code" (string_of_int expected)
          (string_of_int observation.exit_code))
    scenario.expect.exit_code;
  Option.iter
    (fun expected ->
      if observation.stdout <> expected then
        mismatch "expect.stdout"
          (Yojson.Safe.to_string (`String expected))
          (Yojson.Safe.to_string (`String observation.stdout)))
    scenario.expect.stdout;
  Option.iter
    (fun expected ->
      if observation.stderr <> expected then
        mismatch "expect.stderr"
          (Yojson.Safe.to_string (`String expected))
          (Yojson.Safe.to_string (`String observation.stderr)))
    scenario.expect.stderr;
  List.iter
    (fun (expected : expected_file) ->
      let actual =
        match
          List.find_opt
            (fun (file_effect : Observation.file_effect) ->
              file_effect.path = expected.path)
            observation.files
        with
        | None -> None
        | Some file_effect -> file_effect.after
      in
      match actual with
      | Some actual
        when String.lowercase_ascii actual
             = String.lowercase_ascii expected.sha256 ->
          ()
      | Some actual ->
          mismatch ("expect.files." ^ expected.path) expected.sha256 actual
      | None ->
          errors :=
            Printf.sprintf "%s expect.files.%s was not produced" scenario.name
              expected.path
            :: !errors)
    scenario.expect.files;
  List.rev !errors
