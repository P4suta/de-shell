open Deshell

let replace_once ~needle ~replacement value =
  let needle_length = String.length needle in
  let rec find index =
    if index + needle_length > String.length value then None
    else if String.sub value index needle_length = needle then Some index
    else find (index + 1)
  in
  match find 0 with
  | None -> Alcotest.failf "missing fixture text: %s" needle
  | Some index ->
      String.sub value 0 index ^ replacement
      ^ String.sub value (index + needle_length)
          (String.length value - index - needle_length)

let test_default_lock_matches_bundled_model () =
  match Lockfile.decode_string (Lockfile.default ()) with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok lock ->
      Alcotest.(check string)
        "command model digest" (Command_model.digest ())
        lock.command_model_digest;
      begin match Lockfile.observation_image lock with
      | Ok _ ->
          Alcotest.fail "the template must not claim a nonexistent lab image"
      | Error message ->
          Alcotest.(check bool)
            "explicitly unconfigured" true
            (Test_support.contains ~needle:"unconfigured" message)
      end

let test_model_drift_is_rejected () =
  let expected = Command_model.digest () in
  let source =
    Lockfile.default ()
    |> replace_once ~needle:expected ~replacement:(String.make 64 '0')
  in
  match Lockfile.decode_string source with
  | Ok _ -> Alcotest.fail "a stale command model lock was accepted"
  | Error errors ->
      Alcotest.(check bool)
        "drift diagnostic" true
        (List.exists (Test_support.contains ~needle:"command model") errors)

let test_pinned_observation_image () =
  let pinned = "registry.example/deshell/lab@sha256:" ^ String.make 64 'a' in
  let source =
    Lockfile.default ()
    |> replace_once ~needle:{|image = "unconfigured"|}
         ~replacement:(Printf.sprintf {|image = "%s"|} pinned)
  in
  match Lockfile.decode_string source with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok lock ->
      Alcotest.(check (result string string))
        "pinned image" (Ok pinned)
        (Lockfile.observation_image lock)

let test_duplicate_key_is_rejected () =
  let source = Lockfile.default () ^ "\n[lab]\nimage = \"unconfigured\"\n" in
  match Lockfile.decode_string source with
  | Ok _ -> Alcotest.fail "duplicate lock key was accepted"
  | Error errors ->
      Alcotest.(check bool)
        "duplicate diagnostic" true
        (List.exists (Test_support.contains ~needle:"duplicate") errors)

let test_v1_lock_is_migrated_in_memory () =
  let legacy =
    {|version = 1

[toolchain]
ocaml = "5.5.0"
dune = "3.24"
opam = "2.5.2"

[protocol]
adapter = 1
effect_ir = 1
|}
  in
  match Lockfile.decode_string legacy with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok lock ->
      Alcotest.(check int)
        "current layout" Lockfile.current_version lock.version;
      Alcotest.(check (option int))
        "migration source" (Some 1) lock.migrated_from;
      Alcotest.(check string)
        "current model supplied" (Command_model.digest ())
        lock.command_model_digest

let test_failed_observation_blocks_migration () =
  let outcome =
    Observation_run.
      {
        status = Failed;
        provider = Some "podman";
        reason = Some "launcher failed";
        report = None;
      }
  in
  Alcotest.(check bool)
    "failure blocks apply" true
    (Observation_run.blocks_migration outcome)

let () =
  Alcotest.run "Canonical lockfile"
    [
      ( "validation",
        [
          Alcotest.test_case "default model digest" `Quick
            test_default_lock_matches_bundled_model;
          Alcotest.test_case "model drift" `Quick test_model_drift_is_rejected;
          Alcotest.test_case "pinned lab image" `Quick
            test_pinned_observation_image;
          Alcotest.test_case "duplicate key" `Quick
            test_duplicate_key_is_rejected;
          Alcotest.test_case "v1 migration" `Quick
            test_v1_lock_is_migrated_in_memory;
          Alcotest.test_case "failed observation gate" `Quick
            test_failed_observation_blocks_migration;
        ] );
    ]
