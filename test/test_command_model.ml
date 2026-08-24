open Deshell

let has capability classification =
  List.mem capability classification.Command_model.capabilities

let test_known_effects () =
  let curl =
    Command_model.classify
      [ "curl"; "-o"; "artifact"; "https://example.invalid" ]
  in
  Alcotest.(check bool) "known" true curl.known;
  Alcotest.(check bool) "network" true (has Command_model.Network curl);
  Alcotest.(check bool)
    "file write" true
    (has Command_model.Filesystem_write curl);
  Alcotest.(check bool) "not deterministic" false curl.deterministic;
  let date = Command_model.classify [ "date"; "+%s" ] in
  Alcotest.(check bool) "time" true (has Command_model.Time date);
  let random = Command_model.classify [ "openssl"; "rand"; "16" ] in
  Alcotest.(check bool) "random" true (has Command_model.Random random)

let test_unknown_is_conservative () =
  let value = Command_model.classify [ "custom-build-tool" ] in
  Alcotest.(check bool) "unknown" false value.known;
  Alcotest.(check bool) "process" true (has Command_model.Process value);
  Alcotest.(check bool)
    "unknown capability" true
    (has Command_model.Unknown_command value);
  Alcotest.(check bool) "not declared deterministic" false value.deterministic

let test_test_predicates_use_least_privilege () =
  let comparison =
    Command_model.classify [ "test"; "release"; "="; "release" ]
  in
  Alcotest.(check bool) "comparison known" true comparison.known;
  Alcotest.(check bool)
    "string comparison is pure" false
    (has Command_model.Filesystem_read comparison);
  [
    [ "test"; "-f"; "artifact" ];
    [ "test"; "!"; "-d"; "cache" ];
    [ "test"; "left"; "-nt"; "right" ];
    [ "["; "-e"; "artifact"; "]" ];
  ]
  |> List.iter (fun argv ->
      let predicate = Command_model.classify argv in
      Alcotest.(check bool) "file predicate known" true predicate.known;
      Alcotest.(check bool)
        "file predicate reads metadata" true
        (has Command_model.Filesystem_read predicate))

let formal id operation =
  Ir.node ~id ~guarantee:(Ir.Formal { basis = "test" }) operation

let test_plan_annotation () =
  let body =
    formal "root"
      (Ir.Sequence
         [
           formal "compile" (Ir.Exec (Ir.exec [ "printf"; "build" ]));
           formal "download"
             (Ir.Exec (Ir.exec [ "curl"; "https://example.invalid" ]));
           formal "write"
             (Ir.File_write
                { path = "artifact"; contents = "data"; append = false });
         ])
  in
  let plan = Ir.plan ~entrypoint:"main" [ Ir.task ~name:"main" ~body () ] in
  let annotated = Command_model.annotate_plan plan in
  let task = List.hd annotated.tasks in
  Alcotest.(check (list string))
    "capabilities"
    [ "filesystem.write"; "network"; "process" ]
    task.platform_capabilities;
  Alcotest.(check bool) "network task not cacheable" false task.cacheable

let test_model_digest_is_stable () =
  let first = Command_model.digest () in
  let second = Command_model.digest () in
  Alcotest.(check string) "stable" first second;
  Alcotest.(check int) "sha256" 64 (String.length first);
  Alcotest.(check bool)
    "versioned" true
    (Test_support.contains ~needle:"command-model/2"
       (Command_model.lock_entry ()))

let () =
  Alcotest.run "Command model"
    [
      ( "effects",
        [
          Alcotest.test_case "known" `Quick test_known_effects;
          Alcotest.test_case "unknown" `Quick test_unknown_is_conservative;
          Alcotest.test_case "least-privilege test predicates" `Quick
            test_test_predicates_use_least_privilege;
          Alcotest.test_case "task annotation" `Quick test_plan_annotation;
          Alcotest.test_case "digest" `Quick test_model_digest_is_stable;
        ] );
    ]
