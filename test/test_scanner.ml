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

let () =
  Alcotest.run "Repository scanner"
    [
      ( "inventory",
        [
          Alcotest.test_case "known and candidate forms" `Quick
            test_repository_inventory;
        ] );
    ]
