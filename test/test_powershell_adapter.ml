open Deshell

let powershell_program () =
  match Sys.getenv_opt "DESHELL_POWERSHELL_EXE" with
  | Some value -> value
  | None -> "pwsh"

let connect ?(max_bytes = 4 * 1024 * 1024) () =
  let script =
    match Sys.getenv_opt "DESHELL_POWERSHELL_ADAPTER" with
    | Some value -> value
    | None -> Alcotest.fail "DESHELL_POWERSHELL_ADAPTER is not set"
  in
  let policy_arguments =
    if Sys.win32 then [ "-ExecutionPolicy"; "Bypass" ] else []
  in
  match
    Adapter_client.connect_process ~program:(powershell_program ())
      ~arguments:
        ([ "-NoLogo"; "-NoProfile"; "-NonInteractive" ]
        @ policy_arguments @ [ "-File"; script ])
      ~timeout_seconds:10.0 ~max_bytes ()
  with
  | Ok client -> client
  | Error message -> Alcotest.fail message

let call client source =
  match
    Adapter_client.call client ~method_:"frontend.parse"
      ~params:
        (`Assoc
           [
             ("path", `String "build.ps1");
             ("source", `String source);
             ("future", `Bool true);
           ])
  with
  | Ok result -> result
  | Error message -> Alcotest.fail message

let test_official_ast_parser () =
  let client = connect () in
  Fun.protect
    ~finally:(fun () -> Adapter_client.close client)
    (fun () ->
      let server =
        match Adapter_client.handshake client with
        | Ok value -> value
        | Error message -> Alcotest.fail message
      in
      Alcotest.(check bool)
        "parse advertised" true
        (List.mem "frontend.parse" server.capabilities);
      let valid = call client "& 'git' 'status'" in
      Alcotest.(check bool)
        "valid" true
        Yojson.Safe.Util.(valid |> member "valid" |> to_bool);
      Alcotest.(check string)
        "official parser" "System.Management.Automation.Language.Parser"
        Yojson.Safe.Util.(valid |> member "parser" |> to_string);
      Alcotest.(check bool)
        "tokens exposed" true
        (Yojson.Safe.Util.(valid |> member "tokens" |> to_list) <> []);
      let invalid = call client "if (" in
      Alcotest.(check bool)
        "invalid" false
        Yojson.Safe.Util.(invalid |> member "valid" |> to_bool);
      Alcotest.(check bool)
        "diagnostics exposed" true
        (Yojson.Safe.Util.(invalid |> member "diagnostics" |> to_list) <> []))

let test_adapter_rejects_oversized_request () =
  let client = connect ~max_bytes:(8 * 1024 * 1024) () in
  Fun.protect
    ~finally:(fun () -> Adapter_client.close client)
    (fun () ->
      begin match Adapter_client.handshake client with
      | Ok _ -> ()
      | Error message -> Alcotest.fail message
      end;
      match
        Adapter_client.call client ~method_:"frontend.parse"
          ~params:
            (`Assoc
               [
                 ("path", `String "huge.ps1");
                 ("source", `String (String.make (4 * 1024 * 1024) 'a'));
               ])
      with
      | Ok _ -> Alcotest.fail "PowerShell adapter accepted an oversized request"
      | Error message ->
          if not (Test_support.contains ~needle:"byte limit" message) then
            Alcotest.failf "unexpected size diagnostic: %s" message)

let test_static_file_differential_status () =
  Test_support.with_temp_dir @@ fun root ->
  let source =
    "& 'pwsh' '-NoLogo' '-NoProfile' '-NonInteractive' '-Command' 'exit 7'\n\
     $word = 'after'\n\
     & 'pwsh' '-NoLogo' '-NoProfile' '-NonInteractive' '-Command' \
     \"Write-Output $word; exit 0\"\n"
  in
  let script = Filename.concat root "static.ps1" in
  Test_support.write_file script source;
  let original =
    Process_backend.execute
      Runner.
        {
          argv =
            [
              powershell_program ();
              "-NoLogo";
              "-NoProfile";
              "-NonInteractive";
              "-File";
              script;
            ];
          environment = [];
          working_directory = None;
          stdin = "";
        }
  in
  let original =
    match original with
    | Ok value -> value
    | Error message -> Alcotest.fail message
  in
  let lowered = Frontend_registry.lower ~path:"static.ps1" source in
  let backend : Runner.backend =
    {
      execute = Process_backend.execute;
      read_file = (fun _ -> Error "unused");
      write_file = (fun ~path:_ ~contents:_ ~append:_ -> Error "unused");
      remove_file = (fun _ -> Error "unused");
      network_request = (fun ~method_:_ ~uri:_ -> Error "unused");
    }
  in
  let plan =
    Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:lowered.root () ]
  in
  let migrated =
    match Runner.run_plan ~backend ~policy:Runner.default_policy plan with
    | Ok value -> value
    | Error message -> Alcotest.fail message
  in
  Alcotest.(check int) "exit" original.exit_code migrated.exit_code;
  Alcotest.(check string) "stdout" original.stdout migrated.stdout;
  Alcotest.(check string) "stderr" original.stderr migrated.stderr

let () =
  Alcotest.run "PowerShell official adapter"
    [
      ( "parser oracle",
        [
          Alcotest.test_case "official AST" `Quick test_official_ast_parser;
          Alcotest.test_case "message limit" `Quick
            test_adapter_rejects_oversized_request;
          Alcotest.test_case "static file differential status" `Quick
            test_static_file_differential_status;
        ] );
    ]
