open Deshell

let probe ?(commands = []) ?(features = []) ?(rootless = false) () =
  Lab.
    {
      command_exists = (fun command -> List.mem command commands);
      feature_enabled = (fun feature -> List.mem feature features);
      docker_rootless = (fun () -> rootless);
    }

let test_provider_selection () =
  Alcotest.(check bool)
    "podman" true
    (Lab.select ~platform:Linux (probe ~commands:[ "podman" ] ())
    = Ok Lab.Podman);
  Alcotest.(check bool)
    "rootless docker" true
    (Lab.select ~platform:Linux (probe ~commands:[ "docker" ] ~rootless:true ())
    = Ok Lab.Docker_rootless);
  begin match Lab.select ~platform:Linux (probe ~commands:[ "docker" ] ()) with
  | Ok _ -> Alcotest.fail "rootful Docker must not be selected"
  | Error message ->
      Alcotest.(check bool)
        "rootless reason" true
        (Test_support.contains ~needle:"rootless" message)
  end;
  Alcotest.(check bool)
    "Windows Sandbox" true
    (Lab.select ~platform:Windows
       (probe ~features:[ "Containers-DisposableClientVM" ] ())
    = Ok Lab.Windows_sandbox);
  Alcotest.(check bool)
    "Hyper-V fallback" true
    (Lab.select ~platform:Windows
       (probe ~features:[ "Microsoft-Hyper-V-All" ] ())
    = Ok Lab.Hyper_v);
  Alcotest.(check bool)
    "Virtualization.framework" true
    (Lab.select ~platform:Macos (probe ~commands:[ "deshell-vz-agent" ] ())
    = Ok Lab.Virtualization_framework)

let test_forced_provider_is_still_verified () =
  begin match
    Lab.validate_provider ~platform:Linux
      (probe ~commands:[ "docker" ] ())
      Lab.Docker_rootless
  with
  | Ok () -> Alcotest.fail "forcing Docker bypassed the rootless check"
  | Error message ->
      Alcotest.(check bool)
        "rootless diagnostic" true
        (Test_support.contains ~needle:"rootless" message)
  end;
  begin match
    Lab.validate_provider ~platform:Windows
      (probe ~features:[ "Microsoft-Hyper-V-All" ] ())
      Lab.Podman
  with
  | Ok () -> Alcotest.fail "a provider for the wrong host was accepted"
  | Error message ->
      Alcotest.(check bool)
        "platform diagnostic" true
        (Test_support.contains ~needle:"Linux" message)
  end

let request network =
  Lab.
    {
      workspace = "C:/staged/scenario";
      result_path = "C:/staged/result.json";
      interpreter = "sh";
      script = "build.sh";
      args = [ "--check" ];
      environment = [ ("MODE", "test") ];
      timeout_ms = 5000;
      network;
      image =
        "ghcr.io/deshell-lang/lab@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    }

let has value values = List.exists (String.equal value) values

let test_oci_is_hardened_and_digest_pinned () =
  match Lab.launch_spec Lab.Podman (request Lab.Deny) with
  | Error message -> Alcotest.fail message
  | Ok (Lab.Process process) ->
      Alcotest.(check string) "program" "podman" process.program;
      List.iter
        (fun argument ->
          Alcotest.(check bool) argument true (has argument process.argv))
        [
          "--rm";
          "--read-only";
          "--network=none";
          "--cap-drop=ALL";
          "no-new-privileges";
          "--userns=keep-id";
          "--workdir=/workspace";
        ];
      Alcotest.(check bool)
        "digest image" true
        (List.exists
           (String.starts_with ~prefix:"ghcr.io/deshell-lang/lab@sha256:")
           process.argv)
  | Ok _ -> Alcotest.fail "OCI provider must produce a process spec"

let test_windows_sandbox_disables_host_integrations () =
  match Lab.launch_spec Lab.Windows_sandbox (request Lab.Deny) with
  | Error message -> Alcotest.fail message
  | Ok (Lab.Windows_config xml) ->
      List.iter
        (fun needle ->
          Alcotest.(check bool) needle true (Test_support.contains ~needle xml))
        [
          "<Networking>Disable</Networking>";
          "<ClipboardRedirection>Disable</ClipboardRedirection>";
          "<PrinterRedirection>Disable</PrinterRedirection>";
          "<vGPU>Disable</vGPU>";
          "<SandboxFolder>C:\\deshell</SandboxFolder>";
          "shutdown.exe /s /t 0 /f";
          "<ReadOnly>true</ReadOnly>";
        ]
  | Ok _ -> Alcotest.fail "Windows Sandbox must produce a .wsb configuration"

let test_windows_sandbox_uses_encoded_request () =
  let hostile =
    {
      (request Lab.Deny) with
      args = [ "hello & whoami"; "%PATH%" ];
      environment = [ ("TOKEN", "secret & echo leaked") ];
    }
  in
  match Lab.launch_spec Lab.Windows_sandbox hostile with
  | Error message -> Alcotest.fail message
  | Ok (Lab.Windows_config xml) ->
      Alcotest.(check bool)
        "encoded request switch" true
        (Test_support.contains ~needle:"--request-base64" xml);
      List.iter
        (fun plaintext ->
          Alcotest.(check bool)
            ("does not interpolate " ^ plaintext)
            false
            (Test_support.contains ~needle:plaintext xml))
        [ "hello & whoami"; "%PATH%"; "secret & echo leaked" ]
  | Ok _ -> Alcotest.fail "Windows Sandbox must produce a .wsb configuration"

let test_replay_network_is_explicit () =
  let replay =
    Lab.Replay { proxy = "http://10.0.0.2:8080"; tape = "tape.json" }
  in
  match Lab.launch_spec Lab.Docker_rootless (request replay) with
  | Error message -> Alcotest.fail message
  | Ok (Lab.Process process) ->
      Alcotest.(check bool)
        "isolated network" true
        (has "--network=deshell-replay" process.argv);
      Alcotest.(check bool)
        "proxy" true
        (List.exists
           (Test_support.contains ~needle:"HTTP_PROXY=http://10.0.0.2:8080")
           process.argv);
      Alcotest.(check bool)
        "tape" true
        (List.exists
           (Test_support.contains ~needle:"DESHELL_REPLAY_TAPE=tape.json")
           process.argv)
  | Ok _ -> Alcotest.fail "Docker must produce a process spec"

let test_provider_platform_mismatch_is_rejected () =
  match Lab.launch_spec Lab.Hyper_v (request Lab.Deny) with
  | Ok (Lab.Agent_request request) ->
      Alcotest.(check string) "provider" "hyper-v" request.provider;
      Alcotest.(check string) "host writes" "deny" request.host_write;
      Alcotest.(check string) "network" "deny" request.network
  | Ok _ -> Alcotest.fail "Hyper-V uses the guest agent contract"
  | Error message -> Alcotest.fail message

let () =
  Alcotest.run "Disposable lab contract"
    [
      ( "providers",
        [
          Alcotest.test_case "selection" `Quick test_provider_selection;
          Alcotest.test_case "forced provider validation" `Quick
            test_forced_provider_is_still_verified;
          Alcotest.test_case "OCI hardening" `Quick
            test_oci_is_hardened_and_digest_pinned;
          Alcotest.test_case "Windows hardening" `Quick
            test_windows_sandbox_disables_host_integrations;
          Alcotest.test_case "Windows encoded request" `Quick
            test_windows_sandbox_uses_encoded_request;
          Alcotest.test_case "record/replay network" `Quick
            test_replay_network_is_explicit;
          Alcotest.test_case "guest agent policy" `Quick
            test_provider_platform_mismatch_is_rejected;
        ] );
    ]
