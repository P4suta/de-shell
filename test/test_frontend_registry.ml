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
  match (result.root.operation, result.root.guarantee, result.root.source) with
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
    ("build.cmd", "git.exe status", [ "git.exe"; "status" ]);
    ("build.nu", "^git status", [ "git"; "status" ]);
  ]
  |> List.iter (fun (path, source, expected) ->
      expect_exec ~path ~source expected)

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
          Alcotest.test_case "known dynamic residual" `Quick
            test_known_dynamic_syntax_is_residual;
          Alcotest.test_case "trace-only fallback" `Quick
            test_trace_only_fallback;
        ] );
    ]
