open Deshell

let test_detection () =
  let cases =
    [
      ("build.sh", "", "sh");
      ("build.bash", "", "bash");
      ("build.zsh", "", "zsh");
      ("build.fish", "", "fish");
      ("build.ps1", "", "powershell");
      ("build.cmd", "", "cmd");
      ("build.nu", "", "nu");
      ("release", "#!/usr/bin/env zsh\necho ok\n", "zsh");
      ("unknown.automation", "do the thing", "unknown");
    ]
  in
  List.iter
    (fun (path, source, expected) ->
      Alcotest.(check string)
        path expected
        (Frontend_registry.detect ~path ~source))
    cases

let test_posix_static_path () =
  let result = Frontend_registry.lower ~path:"build.bash" "printf ok\n" in
  match result.root.operation with
  | Ir.Exec _ -> ()
  | _ ->
      Alcotest.fail
        "bash literal subset should lower through the POSIX frontend"

let expect_exec ~path ~source expected =
  let result = Frontend_registry.lower ~path source in
  let root =
    match result.root.operation with
    | Ir.Sequence [ command; { operation = Ir.Sequence []; _ } ] -> command
    | _ -> result.root
  in
  match (root.operation, root.guarantee, root.source) with
  | Ir.Exec command, Ir.Formal _, Some span ->
      Alcotest.(check (list string)) path expected command.argv;
      Alcotest.(check string) "source file" path span.file;
      Alcotest.(check int) "source start" 0 span.start_byte;
      Alcotest.(check int) "source end" (String.length source) span.end_byte
  | _ -> Alcotest.fail (path ^ " should lower its static external-call subset")

let test_all_frontend_static_subsets () =
  [
    ("build.zsh", "/usr/bin/printf ok", [ "/usr/bin/printf"; "ok" ]);
    ("build.fish", "command printf ok", [ "printf"; "ok" ]);
    ("build.ps1", "& 'git' 'status'", [ "git"; "status" ]);
    ("build.cmd", "@git.exe status", [ "git.exe"; "status" ]);
    ("build.nu", "^git status", [ "git"; "status" ]);
  ]
  |> List.iter (fun (path, source, expected) ->
      expect_exec ~path ~source expected)

let expect_static_sequence ~path ~source expected =
  let result = Frontend_registry.lower ~path source in
  match (result.root.operation, result.root.guarantee) with
  | Ir.Sequence nodes, Ir.Formal _ ->
      let nodes =
        match List.rev nodes with
        | { Ir.operation = Ir.Sequence []; _ } :: rest -> List.rev rest
        | _ -> nodes
      in
      let actual =
        List.map
          (fun node ->
            match (node.Ir.operation, node.guarantee, node.source) with
            | Ir.Exec command, Ir.Formal _, Some span ->
                (command.argv, span.start_line, span.end_line)
            | _ ->
                Alcotest.fail
                  (path ^ " sequence contains a non-formal command node"))
          nodes
      in
      Alcotest.(check (list (triple (list string) int int)))
        path expected actual
  | _ -> Alcotest.fail (path ^ " should lower to a formal static sequence")

let test_static_multistatement_subsets () =
  [
    ( "build.fish",
      "# generated build\ncommand git status\ncommand printf done\n",
      [ ([ "git"; "status" ], 2, 2); ([ "printf"; "done" ], 3, 3) ] );
    ( "build.ps1",
      "# generated build\n& 'git' 'status'\n& 'tool.exe' '--check'\n",
      [ ([ "git"; "status" ], 2, 2); ([ "tool.exe"; "--check" ], 3, 3) ] );
    ( "build.cmd",
      "@echo off\r\nrem generated build\r\ngit.exe status\r\ntool.com check\r\n",
      [ ([ "git.exe"; "status" ], 3, 3); ([ "tool.com"; "check" ], 4, 4) ] );
  ]
  |> List.iter (fun (path, source, expected) ->
      expect_static_sequence ~path ~source expected)

let test_multistatement_rejects_whole_script_on_dynamic_state () =
  [
    ("build.fish", "command printf before\ncommand printf $VALUE\n");
    ("build.ps1", "& 'tool.exe' 'before'\n& $tool 'after'\n");
    ("build.cmd", "tool.exe before\ntool.exe %VALUE%\n");
  ]
  |> List.iter (fun (path, source) ->
      let result = Frontend_registry.lower ~path source in
      match (result.root.operation, result.root.guarantee) with
      | Ir.Opaque_capsule capsule, Ir.Residual _ ->
          Alcotest.(check string)
            (path ^ " source retained")
            source capsule.source
      | _ ->
          Alcotest.fail
            (path ^ " must not partially lower a state-dependent script"))

let test_cmd_requires_suppressed_command_echo () =
  let source = "git.exe status\r\n" in
  let result = Frontend_registry.lower ~path:"build.cmd" source in
  match (result.root.operation, result.root.guarantee) with
  | Ir.Opaque_capsule capsule, Ir.Residual evidence ->
      Alcotest.(check string) "lossless source" source capsule.source;
      Alcotest.(check bool)
        "actionable reason" true
        (Test_support.contains ~needle:"echo" evidence.reason)
  | _ ->
      Alcotest.fail
        "cmd with command echo enabled is not observationally equivalent to a \
         direct Exec"

let test_powershell_strict_immutable_dataflow () =
  let source =
    "$ErrorActionPreference = 'Stop'\n\
     Set-StrictMode -Version Latest\n\
     $tool = 'tool.exe'\n\
     $mode = $env:BUILD_MODE\n\
     & $tool '--mode' \"$mode\"\n\
     & 'verify.exe' \"artifact-$mode\"\n"
  in
  let result = Frontend_registry.lower ~path:"build.ps1" source in
  match result.root.operation with
  | Ir.Sequence [ first; second; { operation = Ir.Sequence []; _ } ] ->
      begin match first.operation with
      | Ir.Exec command ->
          Alcotest.(check (list string))
            "first argv"
            [ "tool.exe"; "--mode"; "${BUILD_MODE}" ]
            command.argv
      | _ -> Alcotest.fail "first PowerShell statement must be Exec"
      end;
      begin match second.operation with
      | Ir.Exec command ->
          Alcotest.(check (list string))
            "second argv"
            [ "verify.exe"; "artifact-${BUILD_MODE}" ]
            command.argv
      | _ -> Alcotest.fail "second PowerShell statement must be Exec"
      end;
      Alcotest.(check (list string))
        "declared host environment" [ "BUILD_MODE" ]
        (Template.environment_variables result.root)
  | _ -> Alcotest.fail "PowerShell immutable dataflow must lower to a sequence"

let test_powershell_late_immutable_dataflow () =
  let source =
    "$ErrorActionPreference = 'Stop'\n\
     & 'prepare.exe' 'workspace'\n\
     $mode = 'release'\n\
     & 'tool.exe' '--mode' \"$mode\"\n"
  in
  let result = Frontend_registry.lower ~path:"build.ps1" source in
  match result.root.operation with
  | Ir.Sequence [ first; second; { operation = Ir.Sequence []; _ } ] ->
      begin match first.operation with
      | Ir.Exec command ->
          Alcotest.(check (list string))
            "prepare argv"
            [ "prepare.exe"; "workspace" ]
            command.argv
      | _ -> Alcotest.fail "prepare statement must be Exec"
      end;
      begin match second.operation with
      | Ir.Exec command ->
          Alcotest.(check (list string))
            "late constant argv"
            [ "tool.exe"; "--mode"; "release" ]
            command.argv
      | _ -> Alcotest.fail "late constant statement must be Exec"
      end
  | _ -> Alcotest.fail "new late PowerShell constant must lower atomically"

let test_powershell_help_comments_and_header_spelling () =
  let source =
    "<#\n\
     .SYNOPSIS\n\
     The literal example $ignored must not become dataflow.\n\
     #>\n\
     [cmdletbinding()]\n\
     PARAM ()\n\
     set-strictmode -version latest\n\
     $erroractionpreference = 'stop'\n\
     $tool = 'tool.exe'\n\
     & $tool 'build'\n"
  in
  let result = Frontend_registry.lower ~path:"documented.ps1" source in
  match result.root.operation with
  | Ir.Sequence
      [
        { operation = Ir.Exec command; source = Some span; _ };
        { operation = Ir.Sequence []; _ };
      ] ->
      Alcotest.(check (list string))
        "documented argv" [ "tool.exe"; "build" ] command.argv;
      Alcotest.(check int) "command start line" 10 span.start_line;
      Alcotest.(check int) "command end line" 10 span.end_line
  | _ ->
      Alcotest.fail "documented static PowerShell must lower with a source map"

let test_powershell_parameter_and_comment_boundaries () =
  [
    ( "typed parameter block",
      "[CmdletBinding(DefaultParameterSetName = 'Main')]\n\
       param(\n\
      \  [string] $Name = 'default'\n\
       )\n\
       $ErrorActionPreference = 'Stop'\n\
       & 'tool.exe' $Name\n",
      "parameter block" );
    ( "unterminated help comment",
      "<# documentation\n& 'tool.exe' 'must-not-run'\n",
      "unterminated" );
  ]
  |> List.iter (fun (label, source, reason_fragment) ->
      let result = Frontend_registry.lower ~path:"build.ps1" source in
      match (result.root.operation, result.root.guarantee) with
      | Ir.Opaque_capsule capsule, Ir.Residual evidence ->
          Alcotest.(check string) (label ^ " source") source capsule.source;
          Alcotest.(check bool)
            (label ^ " reason") true
            (Test_support.contains ~needle:reason_fragment evidence.reason)
      | _ -> Alcotest.fail (label ^ " must remain a lossless capsule"))

let test_powershell_mutable_or_unknown_state_is_residual () =
  [
    ( "late mutation",
      "$tool = 'tool.exe'\n& $tool first\n$tool = 'other.exe'\n& $tool second\n"
    );
    ("unknown variable", "& $missing value\n");
    ( "assignment after prior reference",
      "& 'tool.exe' $late\n$late = 'value'\n& 'tool.exe' $late\n" );
    ( "native error semantics",
      "$PSNativeCommandUseErrorActionPreference = $true\n& 'tool.exe' value\n"
    );
    ( "member access",
      "$metadata = 'value'\n& 'tool.exe' $metadata.archive_name\n" );
  ]
  |> List.iter (fun (label, source) ->
      let result = Frontend_registry.lower ~path:"build.ps1" source in
      match (result.root.operation, result.root.guarantee) with
      | Ir.Opaque_capsule capsule, Ir.Residual _ ->
          Alcotest.(check string) label source capsule.source
      | _ -> Alcotest.fail (label ^ " must remain an atomic residual"))

let test_nushell_multistatement_requires_runtime_contract () =
  let source = "^first value\n^second value\n" in
  let result = Frontend_registry.lower ~path:"build.nu" source in
  match (result.root.operation, result.root.guarantee) with
  | Ir.Opaque_capsule capsule, Ir.Residual evidence ->
      Alcotest.(check string) "lossless source" source capsule.source;
      Alcotest.(check bool)
        "runtime contract reason" true
        (Test_support.contains ~needle:"multiple" evidence.reason)
  | _ ->
      Alcotest.fail
        "Nushell multiple statements need a pinned runtime status contract"

let test_powershell_file_normal_completion_status () =
  let result =
    Frontend_registry.lower ~path:"build.ps1" "& 'tool.exe' 'build'\n"
  in
  let calls = ref [] in
  let backend : Runner.backend =
    {
      execute =
        (fun request ->
          calls := request.argv :: !calls;
          Ok Runner.{ exit_code = 7; stdout = "output"; stderr = "failure" });
      read_file = (fun _ -> Error "unused");
      write_file = (fun ~path:_ ~contents:_ ~append:_ -> Error "unused");
      remove_file = (fun _ -> Error "unused");
      network_request = (fun ~method_:_ ~uri:_ -> Error "unused");
    }
  in
  let plan =
    Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body:result.root () ]
  in
  begin match Runner.run_plan ~backend ~policy:Runner.default_policy plan with
  | Error message -> Alcotest.fail message
  | Ok observation ->
      Alcotest.(check int)
        "normally completed ps1 status" 0 observation.exit_code;
      Alcotest.(check string)
        "native stdout retained" "output" observation.stdout;
      Alcotest.(check string)
        "native stderr retained" "failure" observation.stderr
  end;
  Alcotest.(check (list (list string)))
    "native command executed once"
    [ [ "tool.exe"; "build" ] ]
    (List.rev !calls)

let test_known_dynamic_syntax_is_residual () =
  [
    ("build.fish", "echo $VALUE");
    ("build.ps1", "Write-Output $env:VALUE");
    ("build.cmd", "tool.exe %VALUE%");
    ("build.nu", "^tool $env.VALUE");
  ]
  |> List.iter (fun (path, source) ->
      let result = Frontend_registry.lower ~path source in
      match (result.root.operation, result.root.guarantee) with
      | Ir.Opaque_capsule capsule, Ir.Residual _ ->
          Alcotest.(check string) "source retained" source capsule.source
      | _ -> Alcotest.fail (path ^ " dynamic syntax must remain residual"))

let test_native_expression_syntax_is_not_literal_argv () =
  [
    ("fish command substitution", "build.fish", "command printf (date)\n");
    ("fish tilde expansion", "build.fish", "command printf ~/artifact\n");
    ( "PowerShell parenthesized expression",
      "build.ps1",
      "& 'tool.exe' (Get-Date)\n" );
    ("PowerShell splatting", "build.ps1", "& 'tool.exe' @arguments\n");
    ("PowerShell parse error", "build.ps1", "& 'tool.exe' 'value')\n");
    ("cmd grouping", "build.cmd", "@tool.exe (value)\r\n");
    ("Nushell subexpression", "build.nu", "^tool (date now)\n");
  ]
  |> List.iter (fun (label, path, source) ->
      let result = Frontend_registry.lower ~path source in
      match (result.root.operation, result.root.guarantee) with
      | Ir.Opaque_capsule capsule, Ir.Residual evidence ->
          Alcotest.(check string) (label ^ " source") source capsule.source;
          Alcotest.(check bool)
            (label ^ " reason") true
            (Test_support.contains ~needle:"expression syntax" evidence.reason)
      | _ ->
          Alcotest.fail
            (label ^ " must not be reinterpreted as a literal external argv"))

let test_literal_frontend_quote_semantics () =
  expect_exec ~path:"quoted.ps1" ~source:"& 'tool.exe' 'it''s'"
    [ "tool.exe"; "it's" ];
  let assigned =
    Frontend_registry.lower ~path:"assigned.ps1"
      "$label = 'it''s $5'\n& 'tool.exe' $label\n"
  in
  begin match assigned.root.operation with
  | Ir.Sequence
      [ { operation = Ir.Exec command; _ }; { operation = Ir.Sequence []; _ } ]
    ->
      Alcotest.(check (list string))
        "single-quoted assignment argv" [ "tool.exe"; "it's $$5" ] command.argv
  | _ -> Alcotest.fail "single-quoted assignment must lower to one Exec"
  end;
  [
    ("fish quoted escape", "quoted.fish", "command printf \"a\\\"b\"\n");
    ("cmd adjacent quotes", "quoted.cmd", "@tool.exe \"a\"\"b\"\r\n");
    ("Nushell quoted escape", "quoted.nu", "^tool \"line\\nvalue\"\n");
  ]
  |> List.iter (fun (label, path, source) ->
      let result = Frontend_registry.lower ~path source in
      match (result.root.operation, result.root.guarantee) with
      | Ir.Opaque_capsule capsule, Ir.Residual _ ->
          Alcotest.(check string) (label ^ " source") source capsule.source
      | _ -> Alcotest.fail (label ^ " must not be decoded with guessed rules"))

let test_trace_only_fallback () =
  let source = "do the thing\n" in
  let result = Frontend_registry.lower ~path:"build.automation" source in
  match (result.root.operation, result.root.guarantee) with
  | Ir.Opaque_capsule capsule, Ir.Residual evidence ->
      Alcotest.(check string) "interpreter" "unknown" capsule.interpreter;
      Alcotest.(check string) "source" source capsule.source;
      Alcotest.(check bool)
        "honest guarantee" true
        (Test_support.contains ~needle:"trace-only" evidence.reason)
  | _ ->
      Alcotest.fail
        "unimplemented frontend must lower to an executable residual"

let () =
  Alcotest.run "Frontend registry"
    [
      ( "dispatch",
        [
          Alcotest.test_case "seven families" `Quick test_detection;
          Alcotest.test_case "POSIX static" `Quick test_posix_static_path;
          Alcotest.test_case "all static subsets" `Quick
            test_all_frontend_static_subsets;
          Alcotest.test_case "static multistatement subsets" `Quick
            test_static_multistatement_subsets;
          Alcotest.test_case "dynamic multistatement is atomic" `Quick
            test_multistatement_rejects_whole_script_on_dynamic_state;
          Alcotest.test_case "cmd command echo boundary" `Quick
            test_cmd_requires_suppressed_command_echo;
          Alcotest.test_case "PowerShell immutable dataflow" `Quick
            test_powershell_strict_immutable_dataflow;
          Alcotest.test_case "PowerShell late immutable dataflow" `Quick
            test_powershell_late_immutable_dataflow;
          Alcotest.test_case "PowerShell help comments and headers" `Quick
            test_powershell_help_comments_and_header_spelling;
          Alcotest.test_case "PowerShell parameter and comment boundaries"
            `Quick test_powershell_parameter_and_comment_boundaries;
          Alcotest.test_case "PowerShell state boundaries" `Quick
            test_powershell_mutable_or_unknown_state_is_residual;
          Alcotest.test_case "PowerShell file completion status" `Quick
            test_powershell_file_normal_completion_status;
          Alcotest.test_case "Nushell multistatement boundary" `Quick
            test_nushell_multistatement_requires_runtime_contract;
          Alcotest.test_case "known dynamic residual" `Quick
            test_known_dynamic_syntax_is_residual;
          Alcotest.test_case "native expression syntax boundary" `Quick
            test_native_expression_syntax_is_not_literal_argv;
          Alcotest.test_case "literal quote semantics" `Quick
            test_literal_frontend_quote_semantics;
          Alcotest.test_case "trace-only fallback" `Quick
            test_trace_only_fallback;
        ] );
    ]
