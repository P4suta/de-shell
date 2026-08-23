open Deshell

let find_effect path effects =
  List.find_opt
    (fun (file_effect : Observation.file_effect) -> file_effect.path = path)
    effects

let test_snapshot_diff_is_deterministic () =
  Test_support.with_temp_dir @@ fun root ->
  let changed = Filename.concat root "changed.txt" in
  let removed = Filename.concat root "removed.txt" in
  Test_support.write_file changed "before";
  Test_support.write_file removed "gone";
  let before =
    match Workspace.capture ~root () with
    | Ok snapshot -> snapshot
    | Error message -> Alcotest.fail message
  in
  Test_support.write_file changed "after";
  Sys.remove removed;
  Test_support.write_file (Filename.concat root "created.txt") "new";
  let after =
    match Workspace.capture ~root () with
    | Ok snapshot -> snapshot
    | Error message -> Alcotest.fail message
  in
  let effects = Workspace.diff ~before ~after in
  Alcotest.(check (list string))
    "sorted paths"
    [ "changed.txt"; "created.txt"; "removed.txt" ]
    (List.map
       (fun (file_effect : Observation.file_effect) -> file_effect.path)
       effects);
  begin match find_effect "changed.txt" effects with
  | Some file_effect ->
      Alcotest.(check bool)
        "before digest" true
        (Option.is_some file_effect.before);
      Alcotest.(check bool)
        "after digest" true
        (Option.is_some file_effect.after)
  | None -> Alcotest.fail "changed file missing"
  end;
  begin match find_effect "removed.txt" effects with
  | Some file_effect ->
      Alcotest.(check bool)
        "removed before" true
        (Option.is_some file_effect.before);
      Alcotest.(check (option string)) "removed after" None file_effect.after
  | None -> Alcotest.fail "removed file missing"
  end

let test_stage_and_materialize_fixtures () =
  Test_support.with_temp_dir @@ fun parent ->
  let source = Filename.concat parent "source" in
  let destination = Filename.concat parent "destination" in
  Unix.mkdir source 0o700;
  Test_support.write_file (Filename.concat source "build.sh") "printf source\n";
  let scenario =
    Scenario.
      {
        name = "fixture";
        args = [];
        environment = [];
        fixtures =
          [
            {
              path = "input/message.txt";
              contents = "fixture\n";
              executable = false;
            };
          ];
        timeout_ms = 1000;
        expect = { exit_code = None; stdout = None; stderr = None; files = [] };
      }
  in
  begin match
    Workspace.stage ~root:source ~destination ~scenario:(Some scenario) ()
  with
  | Error message -> Alcotest.fail message
  | Ok () -> ()
  end;
  Alcotest.(check string)
    "source copied" "printf source\n"
    (Test_support.read_file (Filename.concat destination "build.sh"));
  Alcotest.(check string)
    "fixture written" "fixture\n"
    (Test_support.read_file
       (Filename.concat (Filename.concat destination "input") "message.txt"))

let test_stage_rejects_symlinks () =
  if Sys.win32 then ()
  else
    Test_support.with_temp_dir @@ fun parent ->
    let source = Filename.concat parent "source" in
    let destination = Filename.concat parent "destination" in
    Unix.mkdir source 0o700;
    Test_support.write_file (Filename.concat parent "outside") "secret";
    Unix.symlink
      (Filename.concat parent "outside")
      (Filename.concat source "link");
    match Workspace.stage ~root:source ~destination ~scenario:None () with
    | Ok () -> Alcotest.fail "symlink must not be staged"
    | Error message ->
        Alcotest.(check bool)
          "symlink diagnostic" true
          (Test_support.contains ~needle:"symlink" message)

let test_stage_enforces_total_size_limit () =
  Test_support.with_temp_dir @@ fun parent ->
  let source = Filename.concat parent "source" in
  let destination = Filename.concat parent "destination" in
  Unix.mkdir source 0o700;
  Test_support.write_file (Filename.concat source "large") (String.make 32 'x');
  match
    Workspace.stage ~max_bytes:16 ~root:source ~destination ~scenario:None ()
  with
  | Ok () -> Alcotest.fail "oversized workspace must fail"
  | Error message ->
      Alcotest.(check bool)
        "limit diagnostic" true
        (Test_support.contains ~needle:"16" message)

let () =
  Alcotest.run "Workspace snapshot"
    [
      ( "isolation",
        [
          Alcotest.test_case "snapshot diff" `Quick
            test_snapshot_diff_is_deterministic;
          Alcotest.test_case "stage fixtures" `Quick
            test_stage_and_materialize_fixtures;
          Alcotest.test_case "symlink rejection" `Quick
            test_stage_rejects_symlinks;
          Alcotest.test_case "size limit" `Quick
            test_stage_enforces_total_size_limit;
        ] );
    ]
