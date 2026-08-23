open Deshell

let request argv =
  Runner.{ argv; environment = []; working_directory = None; stdin = "" }

let test_process_execution () =
  let argv =
    if Sys.win32 then [ "cmd"; "/d"; "/s"; "/c"; "echo process-backend" ]
    else [ "printf"; "process-backend" ]
  in
  match Process_backend.execute (request argv) with
  | Error message -> Alcotest.fail message
  | Ok result ->
      Alcotest.(check int) "exit" 0 result.exit_code;
      Alcotest.(check bool)
        "stdout" true
        (Test_support.contains ~needle:"process-backend" result.stdout)

let test_project_backend_starts_process_in_root () =
  Test_support.with_temp_dir @@ fun root ->
  let parent_cwd = Unix.realpath (Sys.getcwd ()) in
  let argv =
    if Sys.win32 then [ "cmd"; "/d"; "/s"; "/c"; "cd" ] else [ "pwd" ]
  in
  let backend = Process_backend.create ~root in
  match backend.execute (request argv) with
  | Error message -> Alcotest.fail message
  | Ok result ->
      Alcotest.(check int) "exit" 0 result.exit_code;
      Alcotest.(check string)
        "child cwd" (Unix.realpath root)
        (Unix.realpath (String.trim result.stdout));
      Alcotest.(check string)
        "parent cwd unchanged" parent_cwd
        (Unix.realpath (Sys.getcwd ()))

let test_filesystem_scope () =
  Test_support.with_temp_dir @@ fun parent ->
  let root = Filename.concat parent "root" in
  Unix.mkdir root 0o700;
  let outside = Filename.concat parent "outside.txt" in
  Test_support.write_file outside "secret";
  let backend = Process_backend.create ~root in
  match backend.read_file "../outside.txt" with
  | Ok _ -> Alcotest.fail "backend read escaped the project root"
  | Error message ->
      Alcotest.(check bool)
        "scope error" true
        (Test_support.contains ~needle:"escapes" message)

let test_write_does_not_follow_escape_symlink () =
  Test_support.with_temp_dir @@ fun parent ->
  let root = Filename.concat parent "root" in
  Unix.mkdir root 0o700;
  let outside = Filename.concat parent "outside.txt" in
  Test_support.write_file outside "secret";
  let backend = Process_backend.create ~root in
  if Sys.win32 then
    match
      backend.write_file ~path:outside ~contents:"changed" ~append:false
    with
    | Ok () -> Alcotest.fail "backend wrote an absolute path outside the root"
    | Error _ -> ()
  else begin
    let link = Filename.concat root "escape.txt" in
    Unix.symlink outside link;
    begin match
      backend.write_file ~path:"escape.txt" ~contents:"changed" ~append:false
    with
    | Ok () -> Alcotest.fail "backend followed a write symlink outside the root"
    | Error message ->
        Alcotest.(check bool)
          "scope error" true
          (Test_support.contains ~needle:"escapes" message)
    end;
    Alcotest.(check string)
      "outside unchanged" "secret"
      (Test_support.read_file outside)
  end

let () =
  Alcotest.run "Platform process backend"
    [
      ( "process",
        [
          Alcotest.test_case "execute" `Quick test_process_execution;
          Alcotest.test_case "project cwd isolation" `Quick
            test_project_backend_starts_process_in_root;
        ] );
      ( "filesystem",
        [
          Alcotest.test_case "root scope" `Quick test_filesystem_scope;
          Alcotest.test_case "write symlink scope" `Quick
            test_write_does_not_follow_escape_symlink;
        ] );
    ]
