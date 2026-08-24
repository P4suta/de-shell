type capability =
  | Process
  | Filesystem_read
  | Filesystem_write
  | Network
  | Time
  | Random
  | Unknown_command

type classification = {
  command : string;
  known : bool;
  capabilities : capability list;
  deterministic : bool;
}

let version = 2

let capability_to_string = function
  | Process -> "process"
  | Filesystem_read -> "filesystem.read"
  | Filesystem_write -> "filesystem.write"
  | Network -> "network"
  | Time -> "time"
  | Random -> "random"
  | Unknown_command -> "unknown-command"

let compare_capability left right =
  String.compare (capability_to_string left) (capability_to_string right)

let normalize_command executable =
  let executable =
    String.map
      (fun character -> if character = '\\' then '/' else character)
      executable
  in
  let basename =
    match List.rev (String.split_on_char '/' executable) with
    | value :: _ -> value
    | [] -> executable
  in
  let basename = String.lowercase_ascii basename in
  if Filename.check_suffix basename ".exe" then
    String.sub basename 0 (String.length basename - 4)
  else basename

let has_any options argv =
  List.exists (fun option -> List.mem option argv) options

let test_reads_files arguments =
  has_any
    [
      "-a";
      "-b";
      "-c";
      "-d";
      "-e";
      "-ef";
      "-f";
      "-g";
      "-G";
      "-h";
      "-k";
      "-L";
      "-nt";
      "-O";
      "-ot";
      "-p";
      "-r";
      "-S";
      "-s";
      "-u";
      "-w";
      "-x";
    ]
    arguments

let known_commands =
  [
    ("printf", [ Process ], true);
    ("echo", [ Process ], true);
    ("true", [ Process ], true);
    ("false", [ Process ], true);
    ("tr", [ Process ], true);
    ("grep", [ Process; Filesystem_read ], true);
    ("sed", [ Process; Filesystem_read ], true);
    ("awk", [ Process; Filesystem_read ], true);
    ("sort", [ Process; Filesystem_read ], true);
    ("head", [ Process; Filesystem_read ], true);
    ("tail", [ Process; Filesystem_read ], true);
    ("cut", [ Process; Filesystem_read ], true);
    ("cat", [ Process; Filesystem_read ], true);
    ("find", [ Process; Filesystem_read ], true);
    ("ls", [ Process; Filesystem_read ], true);
    ("test", [ Process ], true);
    ("[", [ Process ], true);
    ("cp", [ Process; Filesystem_read; Filesystem_write ], true);
    ("mv", [ Process; Filesystem_read; Filesystem_write ], true);
    ("rm", [ Process; Filesystem_write ], true);
    ("mkdir", [ Process; Filesystem_write ], true);
    ("touch", [ Process; Filesystem_write; Time ], false);
    ("chmod", [ Process; Filesystem_write ], true);
    ("curl", [ Process; Network ], false);
    ("wget", [ Process; Network ], false);
    ("git", [ Process; Filesystem_read; Filesystem_write; Network ], false);
    ("date", [ Process; Time ], false);
    ("sleep", [ Process; Time ], false);
    ("openssl", [ Process ], true);
  ]

let classify argv =
  match argv with
  | [] ->
      {
        command = "";
        known = false;
        capabilities = [ Process; Unknown_command ];
        deterministic = false;
      }
  | executable :: arguments ->
      let command = normalize_command executable in
      begin match
        List.find_opt
          (fun (known, _, _) -> String.equal known command)
          known_commands
      with
      | None ->
          {
            command;
            known = false;
            capabilities = [ Process; Unknown_command ];
            deterministic = false;
          }
      | Some (_, base_capabilities, deterministic) ->
          let capabilities =
            if List.mem command [ "test"; "[" ] && test_reads_files arguments
            then Filesystem_read :: base_capabilities
            else if
              command = "curl"
              && has_any [ "-o"; "--output"; "-O"; "--remote-name" ] arguments
            then Filesystem_write :: base_capabilities
            else if command = "wget" && not (List.mem "-O-" arguments) then
              Filesystem_write :: base_capabilities
            else if command = "openssl" then
              match arguments with
              | "rand" :: _ -> Random :: base_capabilities
              | _ -> base_capabilities
            else base_capabilities
          in
          {
            command;
            known = true;
            capabilities = List.sort_uniq compare_capability capabilities;
            deterministic = deterministic && not (List.mem Random capabilities);
          }
      end

let capabilities_of_node node =
  Ir.fold_nodes
    (fun capabilities node ->
      let additions =
        match node.Ir.operation with
        | Ir.Exec command -> (classify command.argv).capabilities
        | Ir.File_read _ -> [ Filesystem_read ]
        | Ir.File_write _ | Ir.File_remove _ -> [ Filesystem_write ]
        | Ir.Network_request _ -> [ Network ]
        | Ir.Opaque_capsule _ -> [ Process; Unknown_command ]
        | Ir.Pipeline _ | Ir.Sequence _ | Ir.Parallel _ | Ir.Condition _
        | Ir.Match _ | Ir.For_each _ | Ir.Try_finally _ | Ir.Task_call _
        | Ir.Set_variable _ | Ir.Capture_stdout _ ->
            []
      in
      List.rev_append additions capabilities)
    [] node
  |> List.sort_uniq compare_capability

let deterministic_node node =
  Ir.fold_nodes
    (fun deterministic node ->
      deterministic
      &&
      match node.Ir.operation with
      | Ir.Exec command -> (classify command.argv).deterministic
      | Ir.Network_request _ | Ir.Opaque_capsule _ -> false
      | Ir.File_read _ | Ir.File_write _ | Ir.File_remove _ | Ir.Pipeline _
      | Ir.Sequence _ | Ir.Parallel _ | Ir.Condition _ | Ir.Match _
      | Ir.For_each _ | Ir.Try_finally _ | Ir.Task_call _ | Ir.Set_variable _
      | Ir.Capture_stdout _ ->
          true)
    true node

let annotate_plan (plan : Ir.plan) =
  let tasks =
    List.map
      (fun (task : Ir.task) ->
        let inferred = capabilities_of_node task.body in
        let capabilities =
          List.map capability_to_string inferred @ task.platform_capabilities
          |> List.sort_uniq String.compare
        in
        let cacheable =
          deterministic_node task.body
          && not
               (List.exists
                  (fun capability ->
                    List.mem capability
                      [ Network; Time; Random; Unknown_command ])
                  inferred)
        in
        { task with platform_capabilities = capabilities; cacheable })
      plan.tasks
  in
  { plan with tasks }

let model_json () =
  `Assoc
    [
      ("version", `Int version);
      ( "commands",
        `List
          (List.map
             (fun (command, capabilities, deterministic) ->
               `Assoc
                 [
                   ("command", `String command);
                   ( "capabilities",
                     `List
                       (capabilities
                       |> List.sort_uniq compare_capability
                       |> List.map (fun capability ->
                           `String (capability_to_string capability))) );
                   ("deterministic", `Bool deterministic);
                 ])
             known_commands) );
    ]

let digest () = model_json () |> Yojson.Safe.to_string |> Sha256.hex

let lock_entry () =
  Printf.sprintf "command-model/%d sha256:%s" version (digest ())
