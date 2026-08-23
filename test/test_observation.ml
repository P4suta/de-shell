open Deshell

let sample =
  Observation.
    {
      exit_code = 0;
      stdout = "done\n";
      stderr = "";
      timed_out = false;
      signal = None;
      processes =
        [ { argv = [ "printf"; "done\\n" ]; exit_code = 0; parent = None } ];
      files =
        [
          {
            path = "out.txt";
            before = None;
            after =
              Some
                "6b3a55e0261b0304143f805a24924d0cde4da60f5c93dcead751c2cc35b455a2";
          };
        ];
      network =
        [
          {
            method_ = "GET";
            uri = "https://example.invalid/data";
            request_digest = Sha256.hex "";
            response_digest = Sha256.hex "body";
            status = 200;
          };
        ];
    }

let test_codec_round_trip () =
  let encoded = Observation.encode_string sample in
  match Observation.decode_string encoded with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok decoded -> Alcotest.(check bool) "round trip" true (decoded = sample)

let test_unknown_fields_are_ignored () =
  let json = Observation.to_yojson sample in
  let extended =
    match json with
    | `Assoc fields -> `Assoc (("future", `Bool true) :: fields)
    | _ -> assert false
  in
  match Observation.of_yojson extended with
  | Error errors -> Alcotest.fail (String.concat "; " errors)
  | Ok decoded -> Alcotest.(check bool) "compatible" true (decoded = sample)

let test_comparison_reports_every_dimension () =
  let actual =
    Observation.
      {
        exit_code = 7;
        stdout = "changed";
        stderr = "warning";
        timed_out = true;
        signal = Some 9;
        processes = [];
        files = [];
        network = [];
      }
  in
  let comparison = Observation.compare ~expected:sample ~actual in
  Alcotest.(check bool) "not equivalent" false comparison.equivalent;
  Alcotest.(check (list string))
    "dimensions"
    [
      "exit_code";
      "stdout";
      "stderr";
      "timeout";
      "signal";
      "process_tree";
      "filesystem";
      "network";
    ]
    (List.map Observation.dimension comparison.differences);
  Alcotest.(check int) "digest" 64 (String.length comparison.actual_digest)

let test_runner_conversion () =
  let runner =
    Runner.
      {
        exit_code = 2;
        stdout = "out";
        stderr = "err";
        trace =
          [
            Process ([ "tool" ], 2);
            File_write "file";
            Network ("POST", "https://example.invalid");
          ];
      }
  in
  let converted = Observation.of_runner runner in
  Alcotest.(check int) "exit" 2 converted.exit_code;
  Alcotest.(check int) "process" 1 (List.length converted.processes);
  Alcotest.(check int) "file" 1 (List.length converted.files);
  Alcotest.(check int) "network" 1 (List.length converted.network)

let () =
  Alcotest.run "Observation contract"
    [
      ( "canonical observation",
        [
          Alcotest.test_case "round trip" `Quick test_codec_round_trip;
          Alcotest.test_case "unknown fields" `Quick
            test_unknown_fields_are_ignored;
          Alcotest.test_case "comparison" `Quick
            test_comparison_reports_every_dimension;
          Alcotest.test_case "runner conversion" `Quick test_runner_conversion;
        ] );
    ]
