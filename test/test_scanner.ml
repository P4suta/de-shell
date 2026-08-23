open Deshell

let find path findings =
  List.find_opt (fun finding -> String.equal finding.Scanner.path path) findings

let test_repository_inventory () =
  Test_support.with_temp_dir @@ fun root ->
  Test_support.write_file
    (Filename.concat root "build.sh")
    "#!/bin/sh\necho build\n";
  Test_support.write_file
    (Filename.concat root "release")
    "#!/usr/bin/env bash\necho release\n";
  Test_support.write_file
    (Filename.concat root "package.json")
    {|{"scripts":{"build":"echo package","native":"node build.js"}}|};
  Test_support.write_file
    (Filename.concat root "custom.yaml")
    "automation: \"echo maybe-shell\"\n";
  Test_support.write_file
    (Filename.concat root "automation.json")
    {|{"hook":"printf json-candidate","description":"ordinary text"}|};
  Test_support.write_file
    (Filename.concat root "automation.toml")
    "hook = \"printf toml-candidate\"\ndescription = \"ordinary text\"\n";
  Test_support.write_file
    (Filename.concat root "build.mk")
    "build:\n\tprintf make-fragment\n";
  Test_support.write_file
    (Filename.concat root "Dockerfile.release")
    {|FROM scratch
RUN printf docker-fragment \
  continued
RUN ["printf", "exec-form"]
|};
  let vscode = Filename.concat root ".vscode" in
  Unix.mkdir vscode 0o700;
  Test_support.write_file
    (Filename.concat vscode "tasks.json")
    "{\n\
    \  // VS Code accepts JSON with comments and trailing commas.\n\
    \  \"version\": \"2.0.0\",\n\
    \  \"tasks\": [\n\
    \    {\"label\":\"build\",\"type\":\"shell\",\"command\":\"./build.sh\"},\n\
    \    {\"label\":\"safe\",\"type\":\"process\",\"command\":\"node\"},\n\
    \  ],\n\
     }\n";
  let github = Filename.concat root ".github" in
  let workflows = Filename.concat github "workflows" in
  Unix.mkdir github 0o700;
  Unix.mkdir workflows 0o700;
  Test_support.write_file
    (Filename.concat workflows "ci.yml")
    "jobs:\n\
    \  build:\n\
    \    steps:\n\
    \      - run: |\n\
    \          echo first\n\
    \          echo second\n";
  Test_support.write_file
    (Filename.concat root ".gitlab-ci.yml")
    "build:\n  script:\n    - echo gitlab-one\n    - ./scripts/build.sh\n";
  Test_support.write_file
    (Filename.concat root "azure-pipelines.yml")
    "steps:\n\
    \  - bash: |\n\
    \      printf azure-bash\n\
    \  - pwsh: Write-Output azure-pwsh\n";
  let circle = Filename.concat root ".circleci" in
  Unix.mkdir circle 0o700;
  Test_support.write_file
    (Filename.concat circle "config.yml")
    "version: 2.1\n\
     jobs:\n\
    \  build:\n\
    \    steps:\n\
    \      - run:\n\
    \          name: Build\n\
    \          command: |\n\
    \            printf circle-command\n";
  Unix.mkdir (Filename.concat root ".git") 0o700;
  Test_support.write_file
    (Filename.concat (Filename.concat root ".git") "ignored.sh")
    "echo ignored\n";
  let findings = Scanner.scan ~root in
  begin match find "build.sh" findings with
  | Some finding ->
      Alcotest.(check bool) "shell file" true (finding.kind = Scanner.Shell_file);
      Alcotest.(check int) "sha256" 64 (String.length finding.content_hash)
  | None -> Alcotest.fail "build.sh was not inventoried"
  end;
  begin match find "release" findings with
  | Some finding ->
      Alcotest.(check (option string))
        "shebang" (Some "bash") finding.interpreter
  | None -> Alcotest.fail "extensionless shebang script was not inventoried"
  end;
  Alcotest.(check bool)
    "package.json script" true
    (List.exists
       (fun (finding : Scanner.finding) ->
         finding.kind = Scanner.Embedded_shell
         && finding.path = "package.json"
         && finding.locator = Some "scripts.build")
       findings);
  Alcotest.(check bool)
    "unknown structured string is only a candidate" true
    (List.exists
       (fun (finding : Scanner.finding) ->
         finding.kind = Scanner.Candidate && finding.path = "custom.yaml")
       findings);
  List.iter
    (fun (path, locator, expected_kind) ->
      Alcotest.(check bool)
        (path ^ " inventory") true
        (List.exists
           (fun (finding : Scanner.finding) ->
             finding.path = path
             && finding.locator = Some locator
             && finding.kind = expected_kind)
           findings))
    [
      ("automation.json", "$.hook", Scanner.Candidate);
      ("automation.toml", "line:1", Scanner.Candidate);
      ("build.mk", "recipe:2", Scanner.Embedded_shell);
      ("Dockerfile.release", "RUN:2", Scanner.Embedded_shell);
      (".vscode/tasks.json", "tasks.0.command", Scanner.Embedded_shell);
      ("azure-pipelines.yml", "bash:2", Scanner.Embedded_shell);
      ("azure-pipelines.yml", "pwsh:4", Scanner.Embedded_shell);
      (".circleci/config.yml", "command:7", Scanner.Embedded_shell);
    ];
  let interpreter path locator =
    findings
    |> List.find_opt (fun (finding : Scanner.finding) ->
        finding.path = path && finding.locator = Some locator)
    |> fun finding -> Option.bind finding (fun finding -> finding.interpreter)
  in
  Alcotest.(check (option string))
    "Azure bash interpreter" (Some "bash")
    (interpreter "azure-pipelines.yml" "bash:2");
  Alcotest.(check (option string))
    "Azure PowerShell interpreter" (Some "powershell")
    (interpreter "azure-pipelines.yml" "pwsh:4");
  Alcotest.(check bool)
    "Docker continuation belongs to one shell fragment" true
    (List.exists
       (fun (finding : Scanner.finding) ->
         finding.path = "Dockerfile.release"
         && finding.locator = Some "RUN:2"
         && finding.content_hash = Sha256.hex "printf docker-fragment continued")
       findings);
  Alcotest.(check bool)
    "Docker exec form is not embedded shell" false
    (List.exists
       (fun (finding : Scanner.finding) ->
         finding.path = "Dockerfile.release" && finding.locator = Some "RUN:4")
       findings);
  Alcotest.(check bool)
    "VS Code process task is not shell" false
    (List.exists
       (fun (finding : Scanner.finding) ->
         finding.path = ".vscode/tasks.json"
         && finding.locator = Some "tasks.1.command")
       findings);
  Alcotest.(check bool)
    "known YAML block shell is inventoried" true
    (List.exists
       (fun (finding : Scanner.finding) ->
         finding.kind = Scanner.Embedded_shell
         && finding.path = ".github/workflows/ci.yml"
         && finding.locator = Some "run:4"
         && finding.content_hash = Sha256.hex "echo first\necho second\n")
       findings);
  Alcotest.(check bool)
    "GitLab script sequence is inventoried" true
    (List.exists
       (fun (finding : Scanner.finding) ->
         finding.kind = Scanner.Embedded_shell
         && finding.path = ".gitlab-ci.yml"
         && finding.locator = Some "script:3"
         && finding.content_hash = Sha256.hex "echo gitlab-one")
       findings
    && List.exists
         (fun (finding : Scanner.finding) ->
           finding.kind = Scanner.Embedded_shell
           && finding.path = ".gitlab-ci.yml"
           && finding.locator = Some "script:4"
           && finding.content_hash = Sha256.hex "./scripts/build.sh")
         findings);
  Alcotest.(check bool)
    "git directory ignored" false
    (List.exists
       (fun (finding : Scanner.finding) ->
         String.starts_with ~prefix:".git/" finding.path)
       findings)

let test_language_inner_attributes_are_not_shebangs () =
  Test_support.with_temp_dir @@ fun root ->
  Test_support.write_file
    (Filename.concat root "windows.rs")
    "#![cfg(windows)]\nfn main() {}\n";
  Test_support.write_file
    (Filename.concat root "fuzz.rs")
    "#![no_main]\nfn fuzz() {}\n";
  Test_support.write_file
    (Filename.concat root "tool.py")
    "#!/usr/bin/env python3\nprint('not shell')\n";
  Test_support.write_file
    (Filename.concat root "release")
    "#!/usr/bin/env -S bash -eu\necho release\n";
  let findings = Scanner.scan ~root in
  List.iter
    (fun path ->
      Alcotest.(check bool)
        (path ^ " is not a shell file")
        false
        (List.exists
           (fun (finding : Scanner.finding) -> finding.path = path)
           findings))
    [ "windows.rs"; "fuzz.rs"; "tool.py" ];
  Alcotest.(check (option string))
    "a real env shebang is still detected" (Some "bash")
    (Option.bind (find "release" findings) (fun finding -> finding.interpreter))

let test_known_pipeline_reports_only_executable_fields () =
  Test_support.with_temp_dir @@ fun root ->
  let github = Filename.concat root ".github" in
  let workflows = Filename.concat github "workflows" in
  let actions = Filename.concat github "actions" in
  let action = Filename.concat actions "verify" in
  Unix.mkdir github 0o700;
  Unix.mkdir workflows 0o700;
  Unix.mkdir actions 0o700;
  Unix.mkdir action 0o700;
  Test_support.write_file
    (Filename.concat workflows "ci.yml")
    {|name: Build ${{ github.ref }}
on: push
jobs:
  build:
    runs-on: windows-latest
    steps:
      - name: Compile coverage >= 90%
        shell: pwsh
        run: |
          Write-Output build
      - uses: example/action@v1
        with:
          command: release
|};
  Test_support.write_file
    (Filename.concat action "action.yml")
    {|name: Verify
description: Verify ${{ inputs.artifact }} >= 1
runs:
  using: composite
  steps:
    - shell: bash
      run: |
        echo composite
|};
  let findings = Scanner.scan ~root in
  Alcotest.(check int) "only run blocks are executable" 2 (List.length findings);
  begin match find ".github/workflows/ci.yml" findings with
  | Some finding ->
      Alcotest.(check bool)
        "run block" true
        (finding.kind = Scanner.Embedded_shell);
      Alcotest.(check (option string))
        "step shell controls the interpreter" (Some "powershell")
        finding.interpreter;
      Alcotest.(check string) "run source" "Write-Output build\n" finding.source
  | None -> Alcotest.fail "workflow run block was not inventoried"
  end;
  begin match find ".github/actions/verify/action.yml" findings with
  | Some finding ->
      Alcotest.(check bool)
        "composite run block" true
        (finding.kind = Scanner.Embedded_shell);
      Alcotest.(check (option string))
        "composite shell" (Some "bash") finding.interpreter;
      Alcotest.(check string)
        "composite source" "echo composite\n" finding.source
  | None -> Alcotest.fail "composite action run block was not inventoried"
  end

let test_structured_candidates_require_executable_context () =
  Test_support.with_temp_dir @@ fun root ->
  Test_support.write_file
    (Filename.concat root "metadata.json")
    {|{
  "description": "echo this is prose",
  "cwd": "${workspaceFolder}",
  "vcs": {"clientKind": "git"},
  "hook": "printf actual-command"
}|};
  Test_support.write_file
    (Filename.concat root "metadata.yaml")
    "description: echo this is prose\nautomation: echo actual-yaml\n";
  Test_support.write_file
    (Filename.concat root "metadata.toml")
    "reason = \"left > right is prose\"\nhook = \"printf actual-toml\"\n";
  let findings = Scanner.scan ~root in
  Alcotest.(check int) "only executable-looking fields" 3 (List.length findings);
  List.iter
    (fun (path, locator, source) ->
      match find path findings with
      | Some finding ->
          Alcotest.(check (option string))
            (path ^ " locator") (Some locator) finding.locator;
          Alcotest.(check string) (path ^ " source") source finding.source
      | None -> Alcotest.fail (path ^ " executable candidate was not found"))
    [
      ("metadata.json", "$.hook", "printf actual-command");
      ("metadata.yaml", "line:2", "echo actual-yaml");
      ("metadata.toml", "line:2", "printf actual-toml");
    ]

let test_vcs_ignored_artifacts_and_lockfiles_are_ignored () =
  Test_support.with_temp_dir @@ fun root ->
  let initialized =
    Test_support.run_process "git" [ "init"; "--quiet"; root ]
  in
  Alcotest.(check int) "temporary git repository" 0 initialized.status;
  Test_support.write_file
    (Filename.concat root ".gitignore")
    "target/\nbuild/\ndist/\nreports/\nmutants.out/\n";
  let generated_directories =
    [ "target"; "build"; "dist"; "reports"; "mutants.out" ]
  in
  List.iter
    (fun name ->
      let directory = Filename.concat root name in
      Unix.mkdir directory 0o700;
      Test_support.write_file
        (Filename.concat directory "generated.sh")
        "#!/bin/sh\necho generated\n")
    generated_directories;
  Test_support.write_file
    (Filename.concat root "pnpm-lock.yaml")
    "engines: {node: '>=20'}\nvitest: '^5 || ^6 || ^7'\n";
  Test_support.write_file
    (Filename.concat root "mutation.json")
    {|{"description":"left == right -> false"}|};
  Alcotest.(check int)
    "generated inputs do not create findings" 0
    (Scanner.scan ~root |> List.length)

let test_host_language_shell_contracts () =
  Test_support.with_temp_dir @@ fun root ->
  let static_cases =
    [
      ( "Build.java",
        {|class Build { void run() throws Exception { new ProcessBuilder("bash", "-c", "echo java").start(); } }|},
        "bash",
        "echo java" );
      ( "build.kt",
        {|fun main() { ProcessBuilder("sh", "-c", "echo kotlin").start() }|},
        "sh",
        "echo kotlin" );
      ( "Build.scala",
        {|object Build { val child = new ProcessBuilder("zsh", "-c", "echo scala").start() }|},
        "zsh",
        "echo scala" );
      ( "build.py",
        {|import subprocess
subprocess.run("echo python", shell=True, check=True)|},
        "platform-shell",
        "echo python" );
      ( "build.js",
        {|const child_process = require("node:child_process");
child_process.exec("echo javascript");|},
        "platform-shell",
        "echo javascript" );
      ( "build.ts",
        {|import { execSync } from "node:child_process";
execSync(`echo typescript`);|},
        "platform-shell",
        "echo typescript" );
      ( "build.go",
        {|func build() { _ = exec.Command("sh", "-c", "echo go").Run() }|},
        "sh",
        "echo go" );
      ( "build.rs",
        {|fn build() { Command::new("bash").arg("-c").arg("echo rust").status(); }|},
        "bash",
        "echo rust" );
      ( "build.c",
        {|int main(void) { return system("echo c"); }|},
        "platform-shell",
        "echo c" );
      ( "build.cpp",
        {|int main() { return std::system("echo cpp"); }|},
        "platform-shell",
        "echo cpp" );
      ( "build.m",
        {|int run(void) { return system("echo objective-c"); }|},
        "platform-shell",
        "echo objective-c" );
      ( "Build.cs",
        {|Process.Start("cmd.exe", "/c echo csharp");|},
        "cmd",
        "echo csharp" );
      ( "Build.fs",
        {|Process.Start("pwsh", "-Command Write-Output fsharp") |> ignore|},
        "powershell",
        "Write-Output fsharp" );
      ( "Build.vb",
        {|Process.Start("cmd.exe", "/c echo visual-basic")|},
        "cmd",
        "echo visual-basic" );
      ( "build.ml",
        {|let () = ignore (Sys.command "echo ocaml")|},
        "platform-shell",
        "echo ocaml" );
      ( "Build.hs",
        {|main = callCommand "echo haskell"|},
        "platform-shell",
        "echo haskell" );
      ( "build.exs",
        {|System.shell("echo elixir")|},
        "platform-shell",
        "echo elixir" );
      ( "build.erl",
        {|run() -> os:cmd("echo erlang").|},
        "platform-shell",
        "echo erlang" );
      ("build.lua", {|os.execute("echo lua")|}, "platform-shell", "echo lua");
      ("build.pl", {|system("echo perl");|}, "platform-shell", "echo perl");
      ("build.rb", {|system("echo ruby")|}, "platform-shell", "echo ruby");
      ( "build.php",
        {|<?php shell_exec("echo php");|},
        "platform-shell",
        "echo php" );
      ("build.R", {|system("echo r")|}, "platform-shell", "echo r");
      ( "build.nim",
        {|discard execShellCmd("echo nim")|},
        "platform-shell",
        "echo nim" );
      ("build.d", {|executeShell("echo d");|}, "platform-shell", "echo d");
      ( "build.clj",
        {|(shell/sh "sh" "-c" "echo clojure")|},
        "sh",
        "echo clojure" );
      ( "build.dart",
        {|Process.run('sh', ['-c', 'echo dart']);|},
        "sh",
        "echo dart" );
      ( "build.groovy",
        {|["sh", "-c", "echo groovy"].execute()|},
        "sh",
        "echo groovy" );
      ("build.jl", {|run(Cmd(["sh", "-c", "echo julia"]))|}, "sh", "echo julia");
      ( "build.zig",
        {|std.process.Child.run(.{ .allocator = allocator, .argv = &.{ "sh", "-c", "echo zig" } });|},
        "sh",
        "echo zig" );
      ( "build.cr",
        {|Process.run("sh", ["-c", "echo crystal"])|},
        "sh",
        "echo crystal" );
    ]
  in
  List.iter
    (fun (path, source, _, _) ->
      Test_support.write_file (Filename.concat root path) source)
    static_cases;
  let dynamic_cases =
    [
      ( "dynamic.py",
        "import subprocess\nsubprocess.run(command, shell=True)\n",
        "platform-shell" );
      ( "dynamic.js",
        "import { exec } from \"node:child_process\";\nexec(command);\n",
        "platform-shell" );
      ( "dynamic.go",
        "func run() { exec.Command(\"sh\", \"-c\", command).Run() }\n",
        "sh" );
      ( "dynamic.erl",
        "run(Signal) -> os:cmd(\"kill \" ++ Signal).\n",
        "platform-shell" );
      ( "dynamic_cmd.go",
        "func run(link string) { exec.Command(\"cmd\", \"/c\", \"mklink\", \
         \"/J\", link).Run() }\n",
        "cmd" );
      ( "interpolated.ts",
        "import { execSync } from \"node:child_process\";\n\
         execSync(`echo ${name}`);\n",
        "platform-shell" );
      ( "interpolated.kt",
        "fun run(name: String) { ProcessBuilder(\"sh\", \"-c\", \"echo \
         $name\").start() }\n",
        "sh" );
      ( "dynamic_args.rs",
        "fn run(marker: &str) { Command::new(\"cmd.exe\").args([\"/c\", \
         \"echo\", marker]); }\n",
        "cmd" );
    ]
  in
  List.iter
    (fun (path, source, _) ->
      Test_support.write_file (Filename.concat root path) source)
    dynamic_cases;
  let negative_cases =
    [
      ("comment.kt", "// ProcessBuilder(\"sh\", \"-c\", \"echo comment\")\n");
      ("literal.py", "example = 'os.system(\"echo string-literal\")'\n");
      ("method.js", "const match = regex.exec(command);\n");
      ("comment.c", "/*\nsystem(\"echo block-comment\");\n*/\n");
      ( "direct.java",
        "class Direct { void run() throws Exception { \
         Runtime.getRuntime().exec(\"git status\"); } }\n" );
      ("direct.go", "func run() { exec.Command(\"git\", \"status\").Run() }\n");
      ( "direct.rs",
        "fn run() { Command::new(\"git\").arg(\"status\").status(); }\n" );
    ]
  in
  List.iter
    (fun (path, source) ->
      Test_support.write_file (Filename.concat root path) source)
    negative_cases;
  let findings = Scanner.scan ~root in
  List.iter
    (fun (path, _, expected_interpreter, expected_source) ->
      let matches =
        List.filter
          (fun (finding : Scanner.finding) -> finding.path = path)
          findings
      in
      Alcotest.(check int) (path ^ " finding count") 1 (List.length matches);
      match matches with
      | [ finding ] ->
          Alcotest.(check bool)
            (path ^ " is embedded") true
            (finding.kind = Scanner.Embedded_shell);
          Alcotest.(check (option string))
            (path ^ " interpreter") (Some expected_interpreter)
            finding.interpreter;
          Alcotest.(check string)
            (path ^ " command") expected_source finding.source;
          Alcotest.(check bool)
            (path ^ " source locator") true
            (Option.fold ~none:false
               ~some:(String.starts_with ~prefix:"source:")
               finding.locator)
      | _ -> ())
    static_cases;
  List.iter
    (fun (path, _, expected_interpreter) ->
      match find path findings with
      | Some finding ->
          Alcotest.(check bool)
            (path ^ " is candidate") true
            (finding.kind = Scanner.Candidate);
          Alcotest.(check (option string))
            (path ^ " interpreter") (Some expected_interpreter)
            finding.interpreter
      | None -> Alcotest.fail (path ^ " dynamic shell call was not inventoried"))
    dynamic_cases;
  List.iter
    (fun (path, _) ->
      Alcotest.(check bool)
        (path ^ " is not shell execution")
        false
        (List.exists
           (fun (finding : Scanner.finding) -> finding.path = path)
           findings))
    negative_cases

let () =
  Alcotest.run "Repository scanner"
    [
      ( "inventory",
        [
          Alcotest.test_case "known and candidate forms" `Quick
            test_repository_inventory;
          Alcotest.test_case "language attributes are not shebangs" `Quick
            test_language_inner_attributes_are_not_shebangs;
          Alcotest.test_case "known pipeline executable fields" `Quick
            test_known_pipeline_reports_only_executable_fields;
          Alcotest.test_case "structured candidate context" `Quick
            test_structured_candidates_require_executable_context;
          Alcotest.test_case "VCS ignored artifacts and lockfiles" `Quick
            test_vcs_ignored_artifacts_and_lockfiles_are_ignored;
          Alcotest.test_case "host-language shell contracts" `Quick
            test_host_language_shell_contracts;
        ] );
    ]
