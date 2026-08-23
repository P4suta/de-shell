type launch = Lab.provider -> Lab.request -> (Observation.t, string) result

let rec remove_tree path =
  if Sys.file_exists path then
    match (Unix.lstat path).Unix.st_kind with
    | Unix.S_DIR ->
        Sys.readdir path
        |> Array.iter (fun name -> remove_tree (Filename.concat path name));
        Unix.rmdir path
    | Unix.S_REG | Unix.S_LNK | Unix.S_CHR | Unix.S_BLK | Unix.S_FIFO
    | Unix.S_SOCK ->
        Sys.remove path

let temporary_directory () =
  let marker = Filename.temp_file "deshell-observation-" ".tmp" in
  Sys.remove marker;
  Unix.mkdir marker 0o700;
  marker

let with_staged_workspace ~root ~scenario action =
  let container = temporary_directory () in
  let workspace = Filename.concat container "workspace" in
  let output = Filename.concat container "output" in
  Fun.protect
    ~finally:(fun () -> try remove_tree container with _ -> ())
    (fun () ->
      match
        Workspace.stage ~root ~destination:workspace ~scenario:(Some scenario)
          ()
      with
      | Error _ as error -> error
      | Ok () ->
          Unix.mkdir output 0o700;
          action ~workspace
            ~result_path:(Filename.concat output "observation.json"))

let original_request ~image ~entry ~interpreter ~scenario ~workspace
    ~result_path =
  Lab.
    {
      workspace;
      result_path;
      interpreter;
      script = entry;
      args = scenario.Scenario.args;
      environment = scenario.environment;
      timeout_ms = scenario.timeout_ms;
      network = Deny;
      image;
    }

let plan_request ~image ~(scenario : Scenario.t) ~workspace ~result_path =
  Lab.
    {
      workspace;
      result_path;
      interpreter = "deshell";
      script = "run";
      args =
        [
          "--root";
          ".";
          "--allow-residual";
          "--allow-file-read";
          "--allow-file-write";
        ]
        @ List.concat_map (fun value -> [ "--arg"; value ]) scenario.args;
      environment = scenario.Scenario.environment;
      timeout_ms = scenario.timeout_ms;
      network = Deny;
      image;
    }

let write_plan workspace plan =
  let directory = Filename.concat workspace ".deshell" in
  Project.ensure_directory directory;
  Project.write_file
    (Filename.concat directory "plan.json")
    (Ir_codec.encode_string plan)

let verify ~(launch : launch) ~provider ~root ~entry ~(plan : Ir.plan)
    ~scenarios ~image =
  match Ir.validate_plan plan with
  | Error errors -> Error (String.concat "; " errors)
  | Ok () ->
      begin match Project.resolve_entry ~root entry with
      | Error _ as error -> error
      | Ok (_, entry_path) ->
          let source = Project.read_file entry_path in
          let interpreter = Frontend_registry.detect ~path:entry ~source in
          let original scenario =
            with_staged_workspace ~root ~scenario
              (fun ~workspace ~result_path ->
                let request =
                  original_request ~image ~entry ~interpreter ~scenario
                    ~workspace ~result_path
                in
                launch provider request)
          in
          let migrated scenario =
            with_staged_workspace ~root ~scenario
              (fun ~workspace ~result_path ->
                write_plan workspace plan;
                let request =
                  plan_request ~image ~scenario ~workspace ~result_path
                in
                launch provider request)
          in
          Ok (Differential.run ~scenarios ~original ~migrated)
      end

let decode_result path =
  try
    if not (Sys.file_exists path) then
      Error ("observer did not produce its result file: " ^ path)
    else
      Observation.decode_string (Project.read_file path)
      |> Result.map_error (String.concat "; ")
  with Sys_error message -> Error message

let launch_process (request : Lab.request) (process : Lab.process) =
  match
    Process_backend.execute
      Runner.
        {
          argv = process.program :: process.argv;
          environment = process.environment;
          working_directory = None;
          stdin = "";
        }
  with
  | Error _ as error -> error
  | Ok result when result.exit_code <> 0 ->
      Error
        (Printf.sprintf "lab launcher exited %d: %s" result.exit_code
           result.stderr)
  | Ok _ -> decode_result request.result_path

let launch_agent (request : Lab.request) (agent : Lab.agent_request) =
  let executable =
    match agent.provider with
    | "hyper-v" -> "deshell-hyperv-agent"
    | "virtualization-framework" -> "deshell-vz-agent"
    | provider -> provider
  in
  match
    Process_backend.execute
      Runner.
        {
          argv = [ executable; "run" ];
          environment = [];
          working_directory = None;
          stdin = Yojson.Safe.to_string agent.payload ^ "\n";
        }
  with
  | Error _ as error -> error
  | Ok result when result.exit_code <> 0 ->
      Error
        (Printf.sprintf "%s exited %d: %s" executable result.exit_code
           result.stderr)
  | Ok result ->
      begin match Observation.decode_string result.stdout with
      | Ok observation -> Ok observation
      | Error errors -> Error (String.concat "; " errors)
      end

let windows_sandbox_executable () =
  let system_root =
    Option.value ~default:"C:\\Windows" (Sys.getenv_opt "SystemRoot")
  in
  let absolute =
    Filename.concat
      (Filename.concat system_root "System32")
      "WindowsSandbox.exe"
  in
  if Sys.file_exists absolute then absolute else "WindowsSandbox.exe"

let launch_windows_config
    ~(execute :
       Runner.process_request -> (Runner.process_result, string) result)
    (request : Lab.request) config =
  let path = Filename.temp_file "deshell-observation-" ".wsb" in
  Fun.protect
    ~finally:(fun () -> try Sys.remove path with Sys_error _ -> ())
    (fun () ->
      Project.write_file path config;
      match
        execute
          Runner.
            {
              argv = [ windows_sandbox_executable (); path ];
              environment = [];
              working_directory = None;
              stdin = "";
            }
      with
      | Error _ as error -> error
      | Ok result when result.exit_code <> 0 ->
          Error
            (Printf.sprintf "Windows Sandbox exited %d: %s" result.exit_code
               result.stderr)
      | Ok _ -> decode_result request.result_path)

let launch_system provider request =
  match Lab.launch_spec provider request with
  | Error _ as error -> error
  | Ok (Lab.Process process) -> launch_process request process
  | Ok (Lab.Agent_request agent) -> launch_agent request agent
  | Ok (Lab.Windows_config config) ->
      launch_windows_config ~execute:Process_backend.execute request config
