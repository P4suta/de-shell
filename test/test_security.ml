open Deshell

let test_sha256_vectors () =
  Alcotest.(check string)
    "empty" "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    (Sha256.hex "");
  Alcotest.(check string)
    "abc" "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    (Sha256.hex "abc")

let test_transactional_hash_guard () =
  Test_support.with_temp_dir @@ fun root ->
  let path = Filename.concat root "script.sh" in
  Test_support.write_file path "echo old\n";
  let proposal = Atomic_patch.prepare ~path ~replacement:"echo new\n" in
  Test_support.write_file path "echo concurrent\n";
  begin match Atomic_patch.apply proposal with
  | Ok () -> Alcotest.fail "stale patch must not apply"
  | Error message ->
      Alcotest.(check bool)
        "hash diagnostic" true
        (Test_support.contains ~needle:"hash" message)
  end;
  Alcotest.(check string)
    "concurrent content preserved" "echo concurrent\n"
    (Test_support.read_file path)

let test_entrypoint_traversal () =
  Test_support.with_temp_dir @@ fun parent ->
  let root = Filename.concat parent "project" in
  Unix.mkdir root 0o700;
  Test_support.write_file (Filename.concat parent "outside.sh") "echo outside\n";
  match Project.resolve_entry ~root "../outside.sh" with
  | Ok _ -> Alcotest.fail "entrypoint traversal was accepted"
  | Error message ->
      Alcotest.(check bool)
        "scope diagnostic" true
        (Test_support.contains ~needle:"escapes" message)

let test_multi_file_patch_is_all_or_nothing () =
  Test_support.with_temp_dir @@ fun root ->
  let first = Filename.concat root "first.sh" in
  let second = Filename.concat root "second.sh" in
  Test_support.write_file first "first-old\n";
  Test_support.write_file second "second-old\n";
  let first_patch =
    Atomic_patch.prepare ~path:first ~replacement:"first-new\n"
  in
  let second_patch =
    Atomic_patch.prepare ~path:second ~replacement:"second-new\n"
  in
  Test_support.write_file second "second-concurrent\n";
  begin match Atomic_patch.apply_all [ first_patch; second_patch ] with
  | Ok () -> Alcotest.fail "a stale member must reject the transaction"
  | Error message ->
      Alcotest.(check bool)
        "stale diagnostic" true
        (Test_support.contains ~needle:"hash" message)
  end;
  Alcotest.(check string)
    "first untouched" "first-old\n"
    (Test_support.read_file first);
  Alcotest.(check string)
    "concurrent second untouched" "second-concurrent\n"
    (Test_support.read_file second)

let test_multi_file_patch_rejects_duplicate_targets () =
  Test_support.with_temp_dir @@ fun root ->
  let path = Filename.concat root "same.sh" in
  Test_support.write_file path "old\n";
  let patch = Atomic_patch.prepare ~path ~replacement:"new\n" in
  match Atomic_patch.apply_all [ patch; patch ] with
  | Ok () -> Alcotest.fail "duplicate targets are ambiguous"
  | Error message ->
      Alcotest.(check bool)
        "duplicate diagnostic" true
        (Test_support.contains ~needle:"duplicate" message)

let test_transaction_can_create_and_patch_atomically () =
  Test_support.with_temp_dir @@ fun root ->
  let existing = Filename.concat root "caller.txt" in
  let created = Filename.concat root "deshell.nu" in
  Test_support.write_file existing "old caller\n";
  let patch = Atomic_patch.prepare ~path:existing ~replacement:"new caller\n" in
  let create =
    Atomic_patch.prepare_create ~path:created
      ~replacement:"export def main [] {}\n" ~permissions:0o644
  in
  begin match Atomic_patch.apply_all [ create; patch ] with
  | Error message -> Alcotest.fail message
  | Ok () -> ()
  end;
  Alcotest.(check string)
    "caller patched" "new caller\n"
    (Test_support.read_file existing);
  Alcotest.(check string)
    "artifact created" "export def main [] {}\n"
    (Test_support.read_file created)

let test_stale_patch_does_not_create_artifact () =
  Test_support.with_temp_dir @@ fun root ->
  let existing = Filename.concat root "caller.txt" in
  let created = Filename.concat root "deshell.nu" in
  Test_support.write_file existing "old caller\n";
  let patch = Atomic_patch.prepare ~path:existing ~replacement:"new caller\n" in
  let create =
    Atomic_patch.prepare_create ~path:created ~replacement:"artifact\n"
      ~permissions:0o644
  in
  Test_support.write_file existing "concurrent caller\n";
  begin match Atomic_patch.apply_all [ create; patch ] with
  | Ok () -> Alcotest.fail "stale transaction must fail"
  | Error _ -> ()
  end;
  Alcotest.(check bool) "artifact absent" false (Sys.file_exists created);
  Alcotest.(check string)
    "concurrent caller preserved" "concurrent caller\n"
    (Test_support.read_file existing)

let test_init_preserves_user_config () =
  Test_support.with_temp_dir @@ fun root ->
  ignore (Project.init ~root);
  let config = Filename.concat root ".deshell/project.toml" in
  Test_support.write_file config "user = true\n";
  ignore (Project.init ~root);
  Alcotest.(check string)
    "not overwritten" "user = true\n"
    (Test_support.read_file config)

let () =
  Alcotest.run "Security invariants"
    [
      ( "hashing",
        [ Alcotest.test_case "SHA-256 vectors" `Quick test_sha256_vectors ] );
      ( "filesystem",
        [
          Alcotest.test_case "transaction hash" `Quick
            test_transactional_hash_guard;
          Alcotest.test_case "multi-file transaction" `Quick
            test_multi_file_patch_is_all_or_nothing;
          Alcotest.test_case "duplicate transaction target" `Quick
            test_multi_file_patch_rejects_duplicate_targets;
          Alcotest.test_case "create and patch transaction" `Quick
            test_transaction_can_create_and_patch_atomically;
          Alcotest.test_case "stale prevents create" `Quick
            test_stale_patch_does_not_create_artifact;
          Alcotest.test_case "path traversal" `Quick test_entrypoint_traversal;
          Alcotest.test_case "init no overwrite" `Quick
            test_init_preserves_user_config;
        ] );
    ]
