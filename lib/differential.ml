type side = Original | Migrated

type outcome =
  | Equivalent of Observation.comparison
  | Different of Observation.comparison
  | Failed of { side : side; message : string }

type report = {
  verified : bool;
  scenarios : string list;
  results : outcome list;
  digest : string;
}

let side_to_string = function Original -> "original" | Migrated -> "migrated"

let outcome_to_yojson = function
  | Equivalent comparison ->
      `Assoc
        [
          ("outcome", `String "equivalent");
          ("expected_digest", `String comparison.Observation.expected_digest);
          ("actual_digest", `String comparison.actual_digest);
        ]
  | Different comparison ->
      `Assoc
        [
          ("outcome", `String "different");
          ("expected_digest", `String comparison.Observation.expected_digest);
          ("actual_digest", `String comparison.actual_digest);
          ( "dimensions",
            `List
              (List.map
                 (fun difference -> `String (Observation.dimension difference))
                 comparison.differences) );
        ]
  | Failed { side; message } ->
      `Assoc
        [
          ("outcome", `String "failed");
          ("side", `String (side_to_string side));
          ("message", `String message);
        ]

let run ~scenarios ~original ~migrated =
  let run_scenario scenario =
    match original scenario with
    | Error message -> Failed { side = Original; message }
    | Ok expected ->
        let expectation_errors =
          Scenario.expectation_errors scenario expected
        in
        if expectation_errors <> [] then
          Failed
            { side = Original; message = String.concat "; " expectation_errors }
        else
          begin match migrated scenario with
          | Error message -> Failed { side = Migrated; message }
          | Ok actual ->
              let expectation_errors =
                Scenario.expectation_errors scenario actual
              in
              if expectation_errors <> [] then
                Failed
                  {
                    side = Migrated;
                    message = String.concat "; " expectation_errors;
                  }
              else
                let comparison = Observation.compare ~expected ~actual in
                if comparison.equivalent then Equivalent comparison
                else Different comparison
          end
  in
  let results = List.map run_scenario scenarios in
  let scenario_names =
    List.map (fun scenario -> scenario.Scenario.name) scenarios
  in
  let encoded =
    `List
      (List.map2
         (fun name result ->
           `Assoc
             [
               ("scenario", `String name); ("result", outcome_to_yojson result);
             ])
         scenario_names results)
    |> Yojson.Safe.to_string
  in
  {
    verified =
      scenarios <> []
      && List.for_all
           (function Equivalent _ -> true | Different _ | Failed _ -> false)
           results;
    scenarios = scenario_names;
    results;
    digest = Sha256.hex encoded;
  }

let rec map_node_guarantees f (node : Ir.node) =
  let map = map_node_guarantees f in
  let operation =
    match node.operation with
    | Ir.Exec command -> Ir.Exec command
    | Ir.Pipeline nodes -> Ir.Pipeline (List.map map nodes)
    | Ir.Sequence nodes -> Ir.Sequence (List.map map nodes)
    | Ir.Parallel nodes -> Ir.Parallel (List.map map nodes)
    | Ir.Condition { predicate; if_true; if_false } ->
        Ir.Condition
          {
            predicate = map predicate;
            if_true = map if_true;
            if_false = Option.map map if_false;
          }
    | Ir.Match { value; cases; default } ->
        Ir.Match
          {
            value;
            cases = List.map (fun (pattern, body) -> (pattern, map body)) cases;
            default = Option.map map default;
          }
    | Ir.For_each { variable; items; body } ->
        Ir.For_each { variable; items; body = map body }
    | Ir.Try_finally { body; finalizer } ->
        Ir.Try_finally { body = map body; finalizer = map finalizer }
    | Ir.Task_call call -> Ir.Task_call call
    | Ir.Set_variable assignment -> Ir.Set_variable assignment
    | Ir.Capture_stdout capture ->
        Ir.Capture_stdout { capture with body = map capture.body }
    | Ir.File_read path -> Ir.File_read path
    | Ir.File_write write -> Ir.File_write write
    | Ir.File_remove path -> Ir.File_remove path
    | Ir.Network_request request -> Ir.Network_request request
    | Ir.Opaque_capsule capsule -> Ir.Opaque_capsule capsule
  in
  { node with operation; guarantee = f node.guarantee }

let same_scenarios left right =
  List.sort_uniq String.compare left = List.sort_uniq String.compare right

let promote_plan report (plan : Ir.plan) =
  if not report.verified then plan
  else
    let promote = function
      | Ir.Exhaustive { scenarios }
        when same_scenarios scenarios report.scenarios ->
          Ir.Differential
            { scenarios = report.scenarios; observation_digest = report.digest }
      | guarantee -> guarantee
    in
    {
      plan with
      tasks =
        List.map
          (fun (task : Ir.task) ->
            { task with body = map_node_guarantees promote task.Ir.body })
          plan.Ir.tasks;
    }
