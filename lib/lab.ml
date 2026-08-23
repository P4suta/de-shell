type platform = Linux | Macos | Windows

type provider =
  | Podman
  | Docker_rootless
  | Windows_sandbox
  | Hyper_v
  | Virtualization_framework

type probe = {
  command_exists : string -> bool;
  feature_enabled : string -> bool;
  docker_rootless : unit -> bool;
}

type network = Deny | Replay of { proxy : string; tape : string }

type request = {
  workspace : string;
  result_path : string;
  interpreter : string;
  script : string;
  args : string list;
  environment : (string * string) list;
  timeout_ms : int;
  network : network;
  image : string;
}

type process = {
  program : string;
  argv : string list;
  environment : (string * string) list;
}

type agent_request = {
  provider : string;
  host_write : string;
  network : string;
  payload : Yojson.Safe.t;
}

type launch_spec =
  | Process of process
  | Windows_config of string
  | Agent_request of agent_request

let provider_to_string = function
  | Podman -> "podman"
  | Docker_rootless -> "docker-rootless"
  | Windows_sandbox -> "windows-sandbox"
  | Hyper_v -> "hyper-v"
  | Virtualization_framework -> "virtualization-framework"

let command_exists command =
  let path = Option.value ~default:"" (Sys.getenv_opt "PATH") in
  let separators = if Sys.win32 then ';' else ':' in
  let extensions =
    if Sys.win32 then
      Option.value ~default:".COM;.EXE;.BAT;.CMD" (Sys.getenv_opt "PATHEXT")
      |> String.split_on_char ';'
    else [ "" ]
  in
  path
  |> String.split_on_char separators
  |> List.exists (fun directory ->
      List.exists
        (fun extension ->
          let candidate =
            Filename.concat directory
              (if Filename.extension command <> "" then command
               else command ^ String.lowercase_ascii extension)
          in
          Sys.file_exists candidate && not (Sys.is_directory candidate))
        ("" :: extensions))

let docker_is_rootless () =
  try
    let argv =
      [| "docker"; "info"; "--format"; "{{json .SecurityOptions}}" |]
    in
    let channel = Unix.open_process_args_in "docker" argv in
    let output =
      let buffer = Buffer.create 128 in
      begin try
        while true do
          Buffer.add_string buffer (input_line channel);
          Buffer.add_char buffer '\n'
        done
      with End_of_file -> ()
      end;
      Buffer.contents buffer
    in
    let status = Unix.close_process_in channel in
    status = Unix.WEXITED 0
    &&
    let lower = String.lowercase_ascii output in
    let needle = "rootless" in
    let rec search index =
      index + String.length needle <= String.length lower
      && (String.sub lower index (String.length needle) = needle
         || search (index + 1))
    in
    search 0
  with _ -> false

let system_probe () =
  let system_root =
    Option.value ~default:"C:\\Windows" (Sys.getenv_opt "SystemRoot")
  in
  let system32 executable =
    Sys.file_exists
      (Filename.concat (Filename.concat system_root "System32") executable)
  in
  {
    command_exists;
    feature_enabled =
      (function
      | "Containers-DisposableClientVM" ->
          Sys.win32 && system32 "WindowsSandbox.exe"
      | "Microsoft-Hyper-V-All" ->
          Sys.win32 && system32 "vmcompute.exe" && system32 "vmconnect.exe"
      | _ -> false);
    docker_rootless = docker_is_rootless;
  }

let platform_of_host () =
  if Sys.win32 then Windows
  else
    match Sys.os_type with
    | "Unix" ->
        let uname =
          try
            let channel = Unix.open_process_in "uname -s" in
            let value = input_line channel in
            ignore (Unix.close_process_in channel);
            String.lowercase_ascii value
          with _ -> ""
        in
        if uname = "darwin" then Macos else Linux
    | _ -> Linux

let select ~platform probe =
  match platform with
  | Linux ->
      if probe.command_exists "podman" then Ok Podman
      else if probe.command_exists "docker" && probe.docker_rootless () then
        Ok Docker_rootless
      else
        Error
          "no supported rootless OCI runtime is available (install Podman or \
           enable rootless Docker)"
  | Windows ->
      if probe.feature_enabled "Containers-DisposableClientVM" then
        Ok Windows_sandbox
      else if probe.feature_enabled "Microsoft-Hyper-V-All" then Ok Hyper_v
      else
        Error
          "Windows Sandbox or Hyper-V is required for disposable observation"
  | Macos ->
      if probe.command_exists "deshell-vz-agent" then
        Ok Virtualization_framework
      else
        Error
          "the signed deshell-vz-agent is required for \
           Virtualization.framework observation"

let validate_provider ~platform probe provider =
  match (platform, provider) with
  | Linux, Podman ->
      if probe.command_exists "podman" then Ok ()
      else Error "the requested Podman executable is unavailable"
  | Linux, Docker_rootless ->
      if not (probe.command_exists "docker") then
        Error "the requested Docker executable is unavailable"
      else if not (probe.docker_rootless ()) then
        Error "the requested Docker daemon is not running in rootless mode"
      else Ok ()
  | Windows, Windows_sandbox ->
      if probe.feature_enabled "Containers-DisposableClientVM" then Ok ()
      else Error "the requested Windows Sandbox feature is unavailable"
  | Windows, Hyper_v ->
      if probe.feature_enabled "Microsoft-Hyper-V-All" then Ok ()
      else Error "the requested Hyper-V feature is unavailable"
  | Macos, Virtualization_framework ->
      if probe.command_exists "deshell-vz-agent" then Ok ()
      else Error "the signed deshell-vz-agent is unavailable"
  | _, Podman | _, Docker_rootless ->
      Error "the requested OCI provider is supported only on Linux"
  | _, Windows_sandbox | _, Hyper_v ->
      Error "the requested provider is supported only on Windows"
  | _, Virtualization_framework ->
      Error
        "the requested Virtualization.framework provider is supported only on \
         macOS"

let is_hex = function
  | '0' .. '9' | 'a' .. 'f' | 'A' .. 'F' -> true
  | _ -> false

let digest_pinned image =
  match String.index_opt image '@' with
  | None -> false
  | Some index ->
      let suffix = String.sub image index (String.length image - index) in
      index > 0
      && String.for_all
           (function ' ' | '\t' | '\r' | '\n' | '@' -> false | _ -> true)
           (String.sub image 0 index)
      && String.length suffix = 72
      && String.starts_with ~prefix:"@sha256:" suffix
      && String.sub suffix 8 64 |> String.for_all is_hex

let safe_script path =
  let path =
    String.map
      (fun character -> if character = '\\' then '/' else character)
      path
  in
  Filename.is_relative path && path <> ""
  && path |> String.split_on_char '/'
     |> List.for_all (fun component ->
         component <> "" && component <> "." && component <> "..")

let network_name = function Deny -> "deny" | Replay _ -> "record-replay"

let validate (request : request) =
  if request.timeout_ms <= 0 then Error "lab timeout must be greater than zero"
  else if not (safe_script request.script) then
    Error "lab script must be a safe workspace-relative path"
  else if String.trim request.interpreter = "" then
    Error "lab interpreter must not be empty"
  else Ok ()

let environment_arguments environment =
  List.concat_map
    (fun (name, value) -> [ "--env"; name ^ "=" ^ value ])
    environment

let oci_spec program (request : request) =
  if not (digest_pinned request.image) then
    Error "OCI lab image must be pinned by sha256 digest"
  else
    let output_directory = Filename.dirname request.result_path in
    let result_name = Filename.basename request.result_path in
    let network_arguments, replay_environment =
      match request.network with
      | Deny -> ([ "--network=none" ], [])
      | Replay { proxy; tape } ->
          ( [ "--network=deshell-replay" ],
            [
              "--env";
              "HTTP_PROXY=" ^ proxy;
              "--env";
              "HTTPS_PROXY=" ^ proxy;
              "--env";
              "NO_PROXY=";
              "--env";
              "DESHELL_REPLAY_TAPE=" ^ tape;
            ] )
    in
    let argv =
      [
        "run";
        "--rm";
        "--read-only";
        "--cap-drop=ALL";
        "--security-opt";
        "no-new-privileges";
        "--userns=keep-id";
        "--pids-limit=512";
        "--memory=1g";
        "--workdir=/workspace";
      ]
      @ network_arguments
      @ [ "--volume"; request.workspace ^ ":/workspace:ro" ]
      @ [ "--volume"; output_directory ^ ":/deshell-output:rw" ]
      @ replay_environment
      @ environment_arguments request.environment
      @ [
          request.image;
          "deshell-observer-agent";
          "--workspace";
          "/workspace";
          "--result";
          "/deshell-output/" ^ result_name;
          "--timeout-ms";
          string_of_int request.timeout_ms;
          "--interpreter";
          request.interpreter;
          "--script";
          request.script;
          "--";
        ]
      @ request.args
    in
    Ok (Process { program; argv; environment = [] })

let xml_escape value =
  let buffer = Buffer.create (String.length value) in
  String.iter
    (function
      | '&' -> Buffer.add_string buffer "&amp;"
      | '<' -> Buffer.add_string buffer "&lt;"
      | '>' -> Buffer.add_string buffer "&gt;"
      | '"' -> Buffer.add_string buffer "&quot;"
      | '\'' -> Buffer.add_string buffer "&apos;"
      | character -> Buffer.add_char buffer character)
    value;
  Buffer.contents buffer

let observer_agent_path () =
  match Sys.getenv_opt "DESHELL_OBSERVER_AGENT" with
  | Some path when String.trim path <> "" -> path
  | _ ->
      let filename =
        if Sys.win32 then "deshell-observer-agent.exe"
        else "deshell-observer-agent"
      in
      Filename.concat (Filename.dirname Sys.executable_name) filename

let absolute_path path =
  if Filename.is_relative path then Filename.concat (Sys.getcwd ()) path
  else path

let windows_sandbox_spec (request : request) =
  match request.network with
  | Replay _ ->
      Error
        "Windows Sandbox replay networking requires the Hyper-V provider with \
         an isolated proxy switch"
  | Deny ->
      let output_directory = Filename.dirname request.result_path in
      let result_name = Filename.basename request.result_path in
      let observer_agent = observer_agent_path () |> absolute_path in
      let observer_agent_directory = Filename.dirname observer_agent in
      let guest_observer_agent =
        "C:\\deshell\\" ^ Filename.basename observer_agent
      in
      let invocation =
        Observer_agent.
          {
            workspace = "C:\\workspace";
            result_path = "C:\\output\\" ^ result_name;
            timeout_ms = request.timeout_ms;
            interpreter = request.interpreter;
            script = request.script;
            args = request.args;
            environment = request.environment;
          }
      in
      let command =
        "cmd.exe /d /s /c \"\"" ^ guest_observer_agent ^ "\" --request-base64 "
        ^ Observer_agent.encode_invocation invocation
        ^ " & shutdown.exe /s /t 0 /f\""
        |> xml_escape
      in
      Ok
        (Windows_config
           (Printf.sprintf
              "<Configuration>\n\
              \  <vGPU>Disable</vGPU>\n\
              \  <Networking>Disable</Networking>\n\
              \  <AudioInput>Disable</AudioInput>\n\
              \  <VideoInput>Disable</VideoInput>\n\
              \  <ProtectedClient>Enable</ProtectedClient>\n\
              \  <PrinterRedirection>Disable</PrinterRedirection>\n\
              \  <ClipboardRedirection>Disable</ClipboardRedirection>\n\
              \  <MappedFolders>\n\
              \    \
               <MappedFolder><HostFolder>%s</HostFolder><SandboxFolder>C:\\workspace</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>\n\
              \    \
               <MappedFolder><HostFolder>%s</HostFolder><SandboxFolder>C:\\output</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>\n\
              \    \
               <MappedFolder><HostFolder>%s</HostFolder><SandboxFolder>C:\\deshell</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>\n\
              \  </MappedFolders>\n\
              \  <LogonCommand><Command>%s</Command></LogonCommand>\n\
               </Configuration>\n"
              (xml_escape request.workspace)
              (xml_escape output_directory)
              (xml_escape observer_agent_directory)
              command))

let agent_payload (request : request) =
  `Assoc
    [
      ("workspace", `String request.workspace);
      ("result_path", `String request.result_path);
      ("interpreter", `String request.interpreter);
      ("script", `String request.script);
      ("args", `List (List.map (fun value -> `String value) request.args));
      ( "environment",
        `Assoc
          (List.map
             (fun (name, value) -> (name, `String value))
             request.environment) );
      ("timeout_ms", `Int request.timeout_ms);
      ("network", `String (network_name request.network));
    ]

let guest_agent_spec provider (request : request) =
  let provider_name =
    match provider with
    | Hyper_v -> "hyper-v"
    | Virtualization_framework -> "virtualization-framework"
    | Podman | Docker_rootless | Windows_sandbox -> assert false
  in
  Ok
    (Agent_request
       {
         provider = provider_name;
         host_write = "deny";
         network = network_name request.network;
         payload = agent_payload request;
       })

let launch_spec provider (request : request) =
  match validate request with
  | Error _ as error -> error
  | Ok () ->
      begin match provider with
      | Podman -> oci_spec "podman" request
      | Docker_rootless -> oci_spec "docker" request
      | Windows_sandbox -> windows_sandbox_spec request
      | Hyper_v | Virtualization_framework -> guest_agent_spec provider request
      end
