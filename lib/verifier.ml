type audit_report = {
  formal : int;
  exhaustive : int;
  differential : int;
  residual : int;
  residual_reasons : string list;
}

type difference =
  | Exit_code of { expected : int; actual : int }
  | Stdout of { expected : string; actual : string }
  | Stderr of { expected : string; actual : string }
  | Trace of {
      expected : Runner.trace_event list;
      actual : Runner.trace_event list;
    }

type comparison = {
  equivalent : bool;
  differences : difference list;
  observation_digest : string;
}

let audit plan =
  match Ir.validate_plan plan with
  | Error errors -> Error errors
  | Ok () ->
      let initial =
        {
          formal = 0;
          exhaustive = 0;
          differential = 0;
          residual = 0;
          residual_reasons = [];
        }
      in
      let report =
        List.fold_left
          (fun report task ->
            Ir.fold_nodes
              (fun report node ->
                match node.Ir.guarantee with
                | Ir.Formal _ -> { report with formal = report.formal + 1 }
                | Ir.Exhaustive _ ->
                    { report with exhaustive = report.exhaustive + 1 }
                | Ir.Differential _ ->
                    { report with differential = report.differential + 1 }
                | Ir.Residual { reason } ->
                    {
                      report with
                      residual = report.residual + 1;
                      residual_reasons =
                        (node.id ^ ": " ^ reason) :: report.residual_reasons;
                    })
              report task.Ir.body)
          initial plan.Ir.tasks
      in
      Ok { report with residual_reasons = List.rev report.residual_reasons }

let trace_event_to_yojson = function
  | Runner.Process (argv, exit_code) ->
      `Assoc
        [
          ("type", `String "process");
          ("argv", `List (List.map (fun value -> `String value) argv));
          ("exit_code", `Int exit_code);
        ]
  | Runner.File_read path ->
      `Assoc [ ("type", `String "file_read"); ("path", `String path) ]
  | Runner.File_write path ->
      `Assoc [ ("type", `String "file_write"); ("path", `String path) ]
  | Runner.File_remove path ->
      `Assoc [ ("type", `String "file_remove"); ("path", `String path) ]
  | Runner.Network (method_, uri) ->
      `Assoc
        [
          ("type", `String "network");
          ("method", `String method_);
          ("uri", `String uri);
        ]
  | Runner.Capsule id ->
      `Assoc [ ("type", `String "capsule"); ("id", `String id) ]

let observation_to_yojson (observation : Runner.observation) =
  `Assoc
    [
      ("exit_code", `Int observation.exit_code);
      ("stdout", `String observation.stdout);
      ("stderr", `String observation.stderr);
      ("trace", `List (List.map trace_event_to_yojson observation.trace));
    ]

let dimension = function
  | Exit_code _ -> "exit_code"
  | Stdout _ -> "stdout"
  | Stderr _ -> "stderr"
  | Trace _ -> "trace"

let compare_observations (expected : Runner.observation)
    (actual : Runner.observation) =
  let differences = ref [] in
  if expected.exit_code <> actual.exit_code then
    differences :=
      Exit_code { expected = expected.exit_code; actual = actual.exit_code }
      :: !differences;
  if expected.stdout <> actual.stdout then
    differences :=
      Stdout { expected = expected.stdout; actual = actual.stdout }
      :: !differences;
  if expected.stderr <> actual.stderr then
    differences :=
      Stderr { expected = expected.stderr; actual = actual.stderr }
      :: !differences;
  if expected.trace <> actual.trace then
    differences :=
      Trace { expected = expected.trace; actual = actual.trace } :: !differences;
  let differences = List.rev !differences in
  let observation_digest =
    actual |> observation_to_yojson |> Yojson.Safe.to_string |> Sha256.hex
  in
  { equivalent = differences = []; differences; observation_digest }
