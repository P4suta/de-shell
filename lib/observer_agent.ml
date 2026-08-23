type invocation = {
  workspace : string;
  result_path : string;
  timeout_ms : int;
  interpreter : string;
  script : string;
  args : string list;
  environment : (string * string) list;
}

let encode_invocation invocation =
  `Assoc
    [
      ("workspace", `String invocation.workspace);
      ("result_path", `String invocation.result_path);
      ("timeout_ms", `Int invocation.timeout_ms);
      ("interpreter", `String invocation.interpreter);
      ("script", `String invocation.script);
      ("args", `List (List.map (fun value -> `String value) invocation.args));
      ( "environment",
        `Assoc
          (List.map
             (fun (key, value) -> (key, `String value))
             invocation.environment) );
    ]
  |> Yojson.Safe.to_string |> Base64.encode

let decode_invocation encoded =
  let ( let* ) result continuation =
    match result with
    | Ok value -> continuation value
    | Error _ as error -> error
  in
  let* source = Base64.decode encoded in
  try
    let fields =
      match Yojson.Safe.from_string source with
      | `Assoc fields -> Ok fields
      | _ -> Error "observer invocation must be a JSON object"
    in
    let* fields = fields in
    let required name =
      match List.assoc_opt name fields with
      | Some value -> Ok value
      | None -> Error ("observer invocation is missing " ^ name)
    in
    let string name =
      let* value = required name in
      match value with
      | `String value when value <> "" -> Ok value
      | _ ->
          Error ("observer invocation " ^ name ^ " must be a non-empty string")
    in
    let strings name =
      let* value = required name in
      match value with
      | `List values ->
          let rec loop accumulator = function
            | [] -> Ok (List.rev accumulator)
            | `String value :: rest -> loop (value :: accumulator) rest
            | _ ->
                Error ("observer invocation " ^ name ^ " must be a string array")
          in
          loop [] values
      | _ -> Error ("observer invocation " ^ name ^ " must be an array")
    in
    let* workspace = string "workspace" in
    let* result_path = string "result_path" in
    let* interpreter = string "interpreter" in
    let* script = string "script" in
    let* args = strings "args" in
    let* timeout = required "timeout_ms" in
    let* timeout_ms =
      match timeout with
      | `Int value when value > 0 -> Ok value
      | _ -> Error "observer invocation timeout_ms must be a positive integer"
    in
    let* environment_value = required "environment" in
    let* environment =
      match environment_value with
      | `Assoc values ->
          let seen = Hashtbl.create (List.length values) in
          let rec loop accumulator = function
            | [] -> Ok (List.rev accumulator)
            | (name, `String value) :: rest when name <> "" ->
                if Hashtbl.mem seen name then
                  Error ("duplicate observer environment variable: " ^ name)
                else begin
                  Hashtbl.add seen name ();
                  loop ((name, value) :: accumulator) rest
                end
            | _ -> Error "observer invocation environment must contain strings"
          in
          loop [] values
      | _ -> Error "observer invocation environment must be an object"
    in
    Ok
      {
        workspace;
        result_path;
        timeout_ms;
        interpreter;
        script;
        args;
        environment;
      }
  with Yojson.Json_error message ->
    Error ("invalid observer invocation JSON: " ^ message)

type request = {
  cwd : string;
  argv : string list;
  environment : (string * string) list;
  timeout_ms : int;
}

type process_observation = {
  exit_code : int;
  stdout : string;
  stderr : string;
  timed_out : bool;
  signal : int option;
  processes : Observation.process list;
  network : Observation.network_effect list;
}

let run ~(execute : request -> (process_observation, string) result) ~root ~argv
    ~environment ~timeout_ms =
  if argv = [] then Error "observer argv must not be empty"
  else if timeout_ms <= 0 then
    Error "observer timeout_ms must be greater than zero"
  else
    try
      let root = Unix.realpath root in
      if not (Sys.is_directory root) then
        Error ("observer workspace is not a directory: " ^ root)
      else
        match Workspace.capture ~root () with
        | Error _ as error -> error
        | Ok before ->
            begin match
              execute { cwd = root; argv; environment; timeout_ms }
            with
            | Error _ as error -> error
            | Ok process ->
                begin match Workspace.capture ~root () with
                | Error _ as error -> error
                | Ok after ->
                    Ok
                      Observation.
                        {
                          exit_code = process.exit_code;
                          stdout = process.stdout;
                          stderr = process.stderr;
                          timed_out = process.timed_out;
                          signal = process.signal;
                          processes = process.processes;
                          files = Workspace.diff ~before ~after;
                          network = process.network;
                        }
                end
            end
    with
    | Sys_error message -> Error message
    | Unix.Unix_error (error, function_name, argument) ->
        Error
          (Printf.sprintf "%s(%s): %s" function_name argument
             (Unix.error_message error))

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let execute_system (request : request) =
  match request.argv with
  | [] -> Error "observer argv must not be empty"
  | executable :: _ ->
      let temporary_paths =
        [
          Filename.temp_file "deshell-agent-stdin-" ".bin";
          Filename.temp_file "deshell-agent-stdout-" ".bin";
          Filename.temp_file "deshell-agent-stderr-" ".bin";
        ]
      in
      let stdin_path, stdout_path, stderr_path =
        match temporary_paths with
        | [ stdin_path; stdout_path; stderr_path ] ->
            (stdin_path, stdout_path, stderr_path)
        | _ -> assert false
      in
      let descriptors = ref [] in
      let close_descriptors () =
        List.iter
          (fun descriptor -> try Unix.close descriptor with _ -> ())
          !descriptors;
        descriptors := []
      in
      let cleanup () =
        close_descriptors ();
        List.iter
          (fun path ->
            if Sys.file_exists path then try Sys.remove path with _ -> ())
          temporary_paths
      in
      let open_tracked path flags permissions =
        let descriptor = Unix.openfile path flags permissions in
        descriptors := descriptor :: !descriptors;
        descriptor
      in
      begin try
        let actual_cwd = Unix.realpath (Sys.getcwd ()) in
        let requested_cwd = Unix.realpath request.cwd in
        if actual_cwd <> requested_cwd then
          failwith
            (Printf.sprintf
               "observer agent process must start in its workspace (expected \
                %s, found %s)"
               requested_cwd actual_cwd);
        let stdin_fd = open_tracked stdin_path [ Unix.O_RDONLY ] 0 in
        let stdout_fd =
          open_tracked stdout_path [ Unix.O_WRONLY; Unix.O_TRUNC ] 0o600
        in
        let stderr_fd =
          open_tracked stderr_path [ Unix.O_WRONLY; Unix.O_TRUNC ] 0o600
        in
        let argv = Array.of_list request.argv in
        let environment =
          Process_backend.merged_environment request.environment
        in
        let pid =
          Unix.create_process_env executable argv environment stdin_fd stdout_fd
            stderr_fd
        in
        close_descriptors ();
        let deadline =
          Unix.gettimeofday () +. (float_of_int request.timeout_ms /. 1000.)
        in
        let rec wait () =
          let waited, status = Unix.waitpid [ Unix.WNOHANG ] pid in
          if waited <> 0 then (false, status)
          else if Unix.gettimeofday () >= deadline then begin
            Unix.kill pid Sys.sigkill;
            let _, status = Unix.waitpid [] pid in
            (true, status)
          end
          else begin
            Unix.sleepf 0.01;
            wait ()
          end
        in
        let timed_out, status = wait () in
        let status_code, signal =
          match status with
          | Unix.WEXITED code -> (code, None)
          | Unix.WSIGNALED signal | Unix.WSTOPPED signal ->
              (128 + signal, Some signal)
        in
        let exit_code = if timed_out then 124 else status_code in
        let result =
          {
            exit_code;
            stdout = read_file stdout_path;
            stderr = read_file stderr_path;
            timed_out;
            signal = (if timed_out then Some Sys.sigkill else signal);
            processes =
              [ Observation.{ argv = request.argv; exit_code; parent = None } ];
            network = [];
          }
        in
        cleanup ();
        Ok result
      with
      | Failure message | Sys_error message ->
          cleanup ();
          Error message
      | Unix.Unix_error (error, function_name, argument) ->
          cleanup ();
          Error
            (Printf.sprintf "%s(%s): %s" function_name argument
               (Unix.error_message error))
      end

let argv_for_script ~interpreter ~script args =
  match String.lowercase_ascii interpreter with
  | "cmd" ->
      let quote value =
        if
          value <> ""
          && String.for_all
               (function
                 | 'A' .. 'Z'
                 | 'a' .. 'z'
                 | '0' .. '9'
                 | '_' | '-' | '.' | '/' | '\\' | ':' ->
                     true
                 | _ -> false)
               value
        then value
        else "\"" ^ String.concat "\"\"" (String.split_on_char '"' value) ^ "\""
      in
      let command =
        "call " ^ String.concat " " (List.map quote (script :: args))
      in
      [ "cmd"; "/d"; "/s"; "/c"; command ]
  | "powershell" ->
      [
        (if Sys.win32 then "powershell.exe" else "pwsh");
        "-NoLogo";
        "-NoProfile";
        "-NonInteractive";
        "-File";
        script;
      ]
      @ args
  | "pwsh" ->
      [ "pwsh"; "-NoLogo"; "-NoProfile"; "-NonInteractive"; "-File"; script ]
      @ args
  | "nu" | "nushell" -> [ "nu"; script ] @ args
  | executable -> executable :: script :: args
