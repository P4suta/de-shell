let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let write_contents path contents =
  let channel = open_out_bin path in
  Fun.protect
    ~finally:(fun () -> close_out_noerr channel)
    (fun () -> output_string channel contents)

let normalize_key key = if Sys.win32 then String.uppercase_ascii key else key

let merged_environment overrides =
  let table = Hashtbl.create 64 in
  Array.iter
    (fun entry ->
      match String.index_opt entry '=' with
      | None -> ()
      | Some separator ->
          let name = String.sub entry 0 separator in
          Hashtbl.replace table (normalize_key name) entry)
    (Unix.environment ());
  List.iter
    (fun (name, value) ->
      Hashtbl.replace table (normalize_key name) (name ^ "=" ^ value))
    overrides;
  Hashtbl.to_seq_values table |> Array.of_seq

let execute_local (request : Runner.process_request) =
  match request.argv with
  | [] -> Error "cannot execute an empty argv"
  | _ when request.working_directory <> None ->
      Error
        "per-process working_directory is not supported by this platform \
         backend"
  | executable :: _ ->
      let temporary_paths = ref [] in
      let descriptors = ref [] in
      let temporary prefix =
        let path = Filename.temp_file prefix ".bin" in
        temporary_paths := path :: !temporary_paths;
        path
      in
      let close_descriptors () =
        List.iter
          (fun descriptor -> try Unix.close descriptor with _ -> ())
          !descriptors;
        descriptors := []
      in
      let open_tracked path flags permissions =
        let descriptor = Unix.openfile path flags permissions in
        descriptors := descriptor :: !descriptors;
        descriptor
      in
      let cleanup () =
        close_descriptors ();
        List.iter
          (fun path -> try Sys.remove path with _ -> ())
          !temporary_paths
      in
      begin try
        let stdin_path = temporary "deshell-stdin-" in
        let stdout_path = temporary "deshell-stdout-" in
        let stderr_path = temporary "deshell-stderr-" in
        write_contents stdin_path request.stdin;
        let stdin_fd = open_tracked stdin_path [ Unix.O_RDONLY ] 0 in
        let stdout_fd =
          open_tracked stdout_path [ Unix.O_WRONLY; Unix.O_TRUNC ] 0o600
        in
        let stderr_fd =
          open_tracked stderr_path [ Unix.O_WRONLY; Unix.O_TRUNC ] 0o600
        in
        let argv = Array.of_list request.argv in
        let environment = merged_environment request.environment in
        let pid =
          Unix.create_process_env executable argv environment stdin_fd stdout_fd
            stderr_fd
        in
        close_descriptors ();
        let _, status = Unix.waitpid [] pid in
        let exit_code =
          match status with
          | Unix.WEXITED code -> code
          | Unix.WSIGNALED signal | Unix.WSTOPPED signal -> 128 + signal
        in
        let stdout = read_file stdout_path in
        let stderr = read_file stderr_path in
        cleanup ();
        Ok Runner.{ exit_code; stdout; stderr }
      with
      | Sys_error message ->
          cleanup ();
          Error message
      | Unix.Unix_error (error, function_name, argument) ->
          cleanup ();
          Error
            (Printf.sprintf "%s(%s): %s" function_name argument
               (Unix.error_message error))
      end

type agent_invocation = { cwd : string; request : Runner.process_request }

let max_agent_protocol_bytes = 64 * 1024 * 1024

let encode_agent_invocation invocation =
  `Assoc
    [
      ("version", `Int 1);
      ("cwd", `String invocation.cwd);
      ( "argv",
        `List
          (List.map (fun value -> `String value) invocation.request.Runner.argv)
      );
      ( "environment",
        `List
          (List.map
             (fun (name, value) -> `List [ `String name; `String value ])
             invocation.request.environment) );
      ("stdin_base64", `String (Base64.encode invocation.request.stdin));
    ]
  |> Yojson.Safe.to_string

let decode_agent_invocation source =
  let ( let* ) result continuation =
    match result with
    | Ok value -> continuation value
    | Error _ as error -> error
  in
  if String.length source > max_agent_protocol_bytes then
    Error "process-agent request exceeds the byte limit"
  else
    try
      let* fields =
        match Yojson.Safe.from_string source with
        | `Assoc fields -> Ok fields
        | _ -> Error "process-agent request must be a JSON object"
      in
      let required name =
        match List.assoc_opt name fields with
        | Some value -> Ok value
        | None -> Error ("process-agent request is missing " ^ name)
      in
      let* version = required "version" in
      let* () =
        match version with
        | `Int 1 -> Ok ()
        | `Int value ->
            Error
              (Printf.sprintf "unsupported process-agent request version: %d"
                 value)
        | _ -> Error "process-agent request version must be an integer"
      in
      let* cwd = required "cwd" in
      let* cwd =
        match cwd with
        | `String value when String.trim value <> "" -> Ok value
        | _ -> Error "process-agent cwd must be a non-empty string"
      in
      let* argv = required "argv" in
      let* argv =
        match argv with
        | `List values ->
            let rec loop accumulator = function
              | [] when accumulator = [] ->
                  Error "process-agent argv must not be empty"
              | [] -> Ok (List.rev accumulator)
              | `String value :: rest -> loop (value :: accumulator) rest
              | _ -> Error "process-agent argv must contain only strings"
            in
            loop [] values
        | _ -> Error "process-agent argv must be an array"
      in
      let* environment = required "environment" in
      let* environment =
        match environment with
        | `List values ->
            let seen = Hashtbl.create (List.length values) in
            let rec loop accumulator = function
              | [] -> Ok (List.rev accumulator)
              | `List [ `String name; `String value ] :: rest
                when String.trim name <> "" ->
                  let key = normalize_key name in
                  if Hashtbl.mem seen key then
                    Error
                      ("duplicate process-agent environment variable: " ^ name)
                  else begin
                    Hashtbl.add seen key ();
                    loop ((name, value) :: accumulator) rest
                  end
              | _ ->
                  Error
                    "process-agent environment entries must be [name, value] \
                     string pairs"
            in
            loop [] values
        | _ -> Error "process-agent environment must be an array"
      in
      let* stdin_base64 = required "stdin_base64" in
      let* stdin =
        match stdin_base64 with
        | `String value -> Base64.decode value
        | _ -> Error "process-agent stdin_base64 must be a string"
      in
      Ok
        {
          cwd;
          request =
            Runner.{ argv; environment; working_directory = None; stdin };
        }
    with Yojson.Json_error message ->
      Error ("invalid process-agent request JSON: " ^ message)

let encode_agent_result (result : Runner.process_result) =
  `Assoc
    [
      ("version", `Int 1);
      ("exit_code", `Int result.exit_code);
      ("stdout_base64", `String (Base64.encode result.stdout));
      ("stderr_base64", `String (Base64.encode result.stderr));
    ]
  |> Yojson.Safe.to_string

let decode_agent_result source =
  let ( let* ) result continuation =
    match result with
    | Ok value -> continuation value
    | Error _ as error -> error
  in
  if String.length source > max_agent_protocol_bytes then
    Error "process-agent response exceeds the byte limit"
  else
    try
      let* fields =
        match Yojson.Safe.from_string source with
        | `Assoc fields -> Ok fields
        | _ -> Error "process-agent response must be a JSON object"
      in
      let required name =
        match List.assoc_opt name fields with
        | Some value -> Ok value
        | None -> Error ("process-agent response is missing " ^ name)
      in
      let* version = required "version" in
      let* () =
        match version with
        | `Int 1 -> Ok ()
        | _ -> Error "process-agent response has an unsupported version"
      in
      let* exit_code = required "exit_code" in
      let* exit_code =
        match exit_code with
        | `Int value -> Ok value
        | _ -> Error "process-agent exit_code must be an integer"
      in
      let decode_payload name =
        let* value = required name in
        match value with
        | `String value -> Base64.decode value
        | _ -> Error ("process-agent " ^ name ^ " must be a string")
      in
      let* stdout = decode_payload "stdout_base64" in
      let* stderr = decode_payload "stderr_base64" in
      Ok Runner.{ exit_code; stdout; stderr }
    with Yojson.Json_error message ->
      Error ("invalid process-agent response JSON: " ^ message)

let read_channel_limited ?(limit = max_agent_protocol_bytes) channel =
  let buffer = Buffer.create 4096 in
  let chunk = Bytes.create 8192 in
  let rec loop total =
    let count = input channel chunk 0 (Bytes.length chunk) in
    if count = 0 then Ok (Buffer.contents buffer)
    else if total + count > limit then
      Error "process-agent message exceeds the byte limit"
    else begin
      Buffer.add_subbytes buffer chunk 0 count;
      loop (total + count)
    end
  in
  loop 0

let process_agent_executable () =
  match Sys.getenv_opt "DESHELL_PROCESS_AGENT" with
  | Some value when String.trim value <> "" -> value
  | _ ->
      let filename =
        if Sys.win32 then "deshell-process-agent.exe"
        else "deshell-process-agent"
      in
      let sibling =
        Filename.concat (Filename.dirname Sys.executable_name) filename
      in
      if Sys.file_exists sibling then sibling else filename

let execute_via_agent request cwd =
  let invocation =
    { cwd; request = { request with Runner.working_directory = None } }
  in
  let helper_request =
    Runner.
      {
        argv = [ process_agent_executable () ];
        environment = [];
        working_directory = None;
        stdin = encode_agent_invocation invocation;
      }
  in
  match execute_local helper_request with
  | Error _ as error -> error
  | Ok result when result.exit_code <> 0 ->
      Error
        (Printf.sprintf "process agent exited %d: %s" result.exit_code
           result.stderr)
  | Ok result -> decode_agent_result result.stdout

let execute (request : Runner.process_request) =
  match request.working_directory with
  | None -> execute_local request
  | Some cwd -> execute_via_agent request cwd

let normalize_path path =
  let path =
    String.map
      (fun character -> if character = '\\' then '/' else character)
      path
  in
  if Sys.win32 then String.lowercase_ascii path else path

let within ~root path =
  let root = normalize_path root in
  let path = normalize_path path in
  path = root || String.starts_with ~prefix:(root ^ "/") path

let resolve_existing ~root path =
  try
    let candidate =
      if Filename.is_relative path then Filename.concat root path else path
    in
    let canonical = Unix.realpath candidate in
    if within ~root canonical then Ok canonical
    else Error ("filesystem effect escapes project root: " ^ path)
  with Sys_error message | Unix.Unix_error (_, _, message) -> Error message

let resolve_write ~root path =
  try
    let candidate =
      if Filename.is_relative path then Filename.concat root path else path
    in
    let candidate_exists =
      try
        ignore (Unix.lstat candidate);
        true
      with Unix.Unix_error (Unix.ENOENT, _, _) -> false
    in
    if candidate_exists then
      let canonical = Unix.realpath candidate in
      if within ~root canonical then Ok canonical
      else Error ("filesystem effect escapes project root: " ^ path)
    else
      let parent = Unix.realpath (Filename.dirname candidate) in
      if within ~root parent then
        Ok (Filename.concat parent (Filename.basename candidate))
      else Error ("filesystem effect escapes project root: " ^ path)
  with Sys_error message | Unix.Unix_error (_, _, message) -> Error message

let create ~root : Runner.backend =
  let root = Unix.realpath root in
  {
    execute =
      (fun request ->
        let requested_directory =
          Option.value ~default:"." request.Runner.working_directory
        in
        match resolve_existing ~root requested_directory with
        | Error _ as error -> error
        | Ok directory when not (Sys.is_directory directory) ->
            Error
              ("working directory is not a directory: " ^ requested_directory)
        | Ok directory ->
            execute { request with Runner.working_directory = Some directory });
    read_file =
      (fun path ->
        match resolve_existing ~root path with
        | Error _ as error -> error
        | Ok path ->
            begin try Ok (read_file path)
            with Sys_error message -> Error message
            end);
    write_file =
      (fun ~path ~contents ~append ->
        match resolve_write ~root path with
        | Error _ as error -> error
        | Ok path ->
            begin try
              let flags =
                if append then
                  [ Open_wronly; Open_creat; Open_binary; Open_append ]
                else [ Open_wronly; Open_creat; Open_binary; Open_trunc ]
              in
              let channel = open_out_gen flags 0o600 path in
              Fun.protect
                ~finally:(fun () -> close_out_noerr channel)
                (fun () -> output_string channel contents);
              Ok ()
            with Sys_error message -> Error message
            end);
    remove_file =
      (fun path ->
        match resolve_existing ~root path with
        | Error _ as error -> error
        | Ok path ->
            begin try
              Sys.remove path;
              Ok ()
            with Sys_error message -> Error message
            end);
    network_request =
      (fun ~method_:_ ~uri ->
        Error
          ("network backend requires the record/replay proxy and is \
            unavailable for " ^ uri));
  }
