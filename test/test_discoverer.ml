open Deshell

let find_locator inventory path locator =
  List.find_opt
    (fun location ->
      location.Discoverer.path = path && location.locator = locator)
    inventory.Discoverer.locations

let fixture root =
  let scripts = Filename.concat root "scripts" in
  Unix.mkdir scripts 0o700;
  Test_support.write_file
    (Filename.concat scripts "build.sh")
    "#!/bin/sh\nprintf build\n";
  Test_support.write_file
    (Filename.concat root "Makefile")
    "build:\n\t./scripts/build.sh\n";
  Test_support.write_file
    (Filename.concat root "package.json")
    "{\n\
    \  \"scripts\": {\n\
    \    \"build\": \"sh scripts/build.sh\",\n\
    \    \"keep\": \"node keep.js\"\n\
    \  }\n\
     }\n";
  let vscode = Filename.concat root ".vscode" in
  Unix.mkdir vscode 0o700;
  Test_support.write_file
    (Filename.concat vscode "tasks.json")
    "{\n\
    \  \"version\": \"2.0.0\",\n\
    \  \"tasks\": [\n\
    \    {\"label\": \"build\", \"type\": \"shell\", \"command\": \
     \"./scripts/build.sh\"}\n\
    \  ]\n\
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
    \          ./scripts/build.sh\n"

let test_inventory_and_call_graph () =
  Test_support.with_temp_dir @@ fun root ->
  fixture root;
  let inventory = Discoverer.discover ~root in
  let shell =
    match find_locator inventory "scripts/build.sh" None with
    | Some value -> value
    | None -> Alcotest.fail "shell file missing"
  in
  let make =
    match find_locator inventory "Makefile" (Some "recipe:2") with
    | Some value -> value
    | None -> Alcotest.fail "Make callsite missing"
  in
  let package =
    match find_locator inventory "package.json" (Some "scripts.build") with
    | Some value -> value
    | None -> Alcotest.fail "package callsite missing"
  in
  let vscode =
    match
      find_locator inventory ".vscode/tasks.json" (Some "tasks.0.command")
    with
    | Some value -> value
    | None -> Alcotest.fail "VS Code task callsite missing"
  in
  let github =
    match find_locator inventory ".github/workflows/ci.yml" (Some "run:4") with
    | Some value -> value
    | None -> Alcotest.fail "GitHub Actions block callsite missing"
  in
  Alcotest.(check int) "stable id" 64 (String.length make.id);
  List.iter
    (fun caller ->
      Alcotest.(check bool)
        ("edge from " ^ caller.Discoverer.path)
        true
        (List.exists
           (fun edge ->
             edge.Discoverer.caller = caller.id && edge.callee = shell.id)
           inventory.edges))
    [ make; package; vscode; github ];
  Alcotest.(check bool)
    "known forms fully classified" true
    (List.for_all
       (fun location ->
         location.Discoverer.classification <> Discoverer.Candidate)
       inventory.locations)

let replacements inventory =
  inventory.Discoverer.locations
  |> List.filter_map (fun (location : Discoverer.location) ->
      match (location.path, location.locator) with
      | "Makefile", Some "recipe:2" ->
          Some (location.id, "deshell run --node build")
      | "package.json", Some "scripts.build" ->
          Some (location.id, "deshell run --node build")
      | ".vscode/tasks.json", Some "tasks.0.command" ->
          Some (location.id, "deshell run --node build")
      | ".github/workflows/ci.yml", Some "run:4" ->
          Some (location.id, "deshell run --node build")
      | _ -> None)

let test_patch_preview_then_transactional_apply () =
  Test_support.with_temp_dir @@ fun root ->
  fixture root;
  let inventory = Discoverer.discover ~root in
  let make_path = Filename.concat root "Makefile" in
  let package_path = Filename.concat root "package.json" in
  let vscode_path = Filename.concat root ".vscode/tasks.json" in
  let github_path = Filename.concat root ".github/workflows/ci.yml" in
  let before_make = Test_support.read_file make_path in
  let before_package = Test_support.read_file package_path in
  let before_vscode = Test_support.read_file vscode_path in
  let before_github = Test_support.read_file github_path in
  begin match
    Discoverer.patch ~root ~inventory ~replacements:(replacements inventory)
      ~apply:false
  with
  | Error message -> Alcotest.fail message
  | Ok result ->
      Alcotest.(check int) "four files" 4 (List.length result.files);
      Alcotest.(check bool)
        "preview command" true
        (Test_support.contains ~needle:"deshell run --node build" result.preview)
  end;
  Alcotest.(check string)
    "Make untouched" before_make
    (Test_support.read_file make_path);
  Alcotest.(check string)
    "package untouched" before_package
    (Test_support.read_file package_path);
  Alcotest.(check string)
    "VS Code untouched" before_vscode
    (Test_support.read_file vscode_path);
  Alcotest.(check string)
    "GitHub Actions untouched" before_github
    (Test_support.read_file github_path);
  begin match
    Discoverer.patch ~root ~inventory ~replacements:(replacements inventory)
      ~apply:true
  with
  | Error message -> Alcotest.fail message
  | Ok result -> Alcotest.(check bool) "applied" true result.applied
  end;
  Alcotest.(check bool)
    "Make syntax preserved" true
    (Test_support.contains ~needle:"\tdeshell run --node build\n"
       (Test_support.read_file make_path));
  let package = Yojson.Safe.from_file package_path in
  Alcotest.(check string)
    "package script" "deshell run --node build"
    Yojson.Safe.Util.(
      package |> member "scripts" |> member "build" |> to_string);
  Alcotest.(check string)
    "unrelated script" "node keep.js"
    Yojson.Safe.Util.(package |> member "scripts" |> member "keep" |> to_string);
  let vscode = Yojson.Safe.from_file vscode_path in
  Alcotest.(check string)
    "VS Code shell task" "deshell run --node build"
    Yojson.Safe.Util.(
      vscode |> member "tasks" |> index 0 |> member "command" |> to_string);
  Alcotest.(check string)
    "GitHub block replaced without orphaned lines"
    "jobs:\n  build:\n    steps:\n      - run: deshell run --node build\n"
    (Test_support.read_file github_path)

let test_drift_rejects_every_file () =
  Test_support.with_temp_dir @@ fun root ->
  fixture root;
  let inventory = Discoverer.discover ~root in
  let make_path = Filename.concat root "Makefile" in
  let package_path = Filename.concat root "package.json" in
  let original_make = Test_support.read_file make_path in
  Test_support.write_file package_path "{\"scripts\":{\"build\":\"changed\"}}\n";
  begin match
    Discoverer.patch ~root ~inventory ~replacements:(replacements inventory)
      ~apply:true
  with
  | Ok _ -> Alcotest.fail "stale inventory must not apply"
  | Error message ->
      Alcotest.(check bool)
        "hash diagnostic" true
        (Test_support.contains ~needle:"hash" message)
  end;
  Alcotest.(check string)
    "other file untouched" original_make
    (Test_support.read_file make_path)

let test_migration_refuses_non_equivalent_callsite_replacement () =
  Test_support.with_temp_dir @@ fun root ->
  let scripts = Filename.concat root "scripts" in
  Unix.mkdir scripts 0o700;
  Test_support.write_file
    (Filename.concat scripts "build.sh")
    "#!/bin/sh\nprintf build\n";
  Test_support.write_file
    (Filename.concat root "Makefile")
    "build:\n\t./scripts/build.sh && printf keep\n";
  match Migration.prepare ~root ~entry:"scripts/build.sh" ~artifact:None with
  | Ok _ ->
      Alcotest.fail
        "migration silently replaced a composite callsite and dropped behavior"
  | Error message ->
      Alcotest.(check bool)
        "manual diagnostic" true
        (Test_support.contains ~needle:"manual" message
        && Test_support.contains ~needle:"Makefile" message)

let test_host_language_callsite_requires_syntax_aware_patcher () =
  Test_support.with_temp_dir @@ fun root ->
  let path = Filename.concat root "build.py" in
  let source = "import os\nos.system(\"echo build\")\n" in
  Test_support.write_file path source;
  let inventory = Discoverer.discover ~root in
  let location =
    match
      List.find_opt
        (fun (location : Discoverer.location) ->
          location.path = "build.py"
          && Option.fold ~none:false
               ~some:(String.starts_with ~prefix:"source:python:")
               location.locator)
        inventory.locations
    with
    | Some location -> location
    | None -> Alcotest.fail "Python shell callsite missing"
  in
  begin match
    Discoverer.patch ~root ~inventory
      ~replacements:[ (location.id, "deshell run --node build") ]
      ~apply:false
  with
  | Ok _ -> Alcotest.fail "host-language source was patched as YAML"
  | Error message ->
      Alcotest.(check bool)
        "syntax-aware patch diagnostic" true
        (Test_support.contains ~needle:"syntax-aware" message)
  end;
  Alcotest.(check string)
    "source untouched" source
    (Test_support.read_file path)

let () =
  Alcotest.run "Repository discoverer"
    [
      ( "whole repository",
        [
          Alcotest.test_case "inventory/call graph" `Quick
            test_inventory_and_call_graph;
          Alcotest.test_case "preview/apply" `Quick
            test_patch_preview_then_transactional_apply;
          Alcotest.test_case "drift transaction" `Quick
            test_drift_rejects_every_file;
          Alcotest.test_case "composite callsite rejection" `Quick
            test_migration_refuses_non_equivalent_callsite_replacement;
          Alcotest.test_case "host-language patch safety" `Quick
            test_host_language_callsite_requires_syntax_aware_patcher;
        ] );
    ]
