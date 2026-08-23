type corpus_item = {
  name : string;
  plan : Ir.plan;
  non_interactive : bool;
  executable_with_residual : bool;
}

type matrix_entry = { operating_system : string; shell : string; passed : bool }

type report = {
  ready : bool;
  total_nodes : int;
  non_residual_nodes : int;
  non_residual_coverage : float;
  executable_scripts : int;
  non_interactive_scripts : int;
  embedded_found : int;
  embedded_expected : int;
  matrix_passed : int;
  matrix_required : int;
  failures : string list;
}

let operating_systems = [ "linux"; "macos"; "windows" ]

let shells =
  [ "posix-sh"; "bash"; "zsh"; "fish"; "powershell"; "cmd"; "nushell" ]

let required_matrix =
  List.concat_map
    (fun operating_system ->
      List.map (fun shell -> (operating_system, shell)) shells)
    operating_systems

let evaluate ~corpus ~unexplained_differences ~expected_embedded ~found_embedded
    ~matrix =
  let failures = ref [] in
  if corpus = [] then failures := "curated corpus is empty" :: !failures;
  if unexplained_differences <> 0 then
    failures :=
      Printf.sprintf "%d unexplained scenario difference(s) remain"
        unexplained_differences
      :: !failures;
  let total_nodes = ref 0 in
  let non_residual_nodes = ref 0 in
  let non_interactive_scripts = ref 0 in
  let executable_scripts = ref 0 in
  List.iter
    (fun item ->
      begin match Ir.validate_plan item.plan with
      | Ok () -> ()
      | Error errors ->
          failures :=
            Printf.sprintf "corpus item %s has invalid evidence: %s" item.name
              (String.concat "; " errors)
            :: !failures
      end;
      List.iter
        (fun task ->
          Ir.fold_nodes
            (fun () node ->
              incr total_nodes;
              begin match node.Ir.guarantee with
              | Ir.Residual _ -> ()
              | Ir.Formal _ | Ir.Exhaustive _ | Ir.Differential _ ->
                  incr non_residual_nodes
              end)
            () task.Ir.body)
        item.plan.tasks;
      if item.non_interactive then begin
        incr non_interactive_scripts;
        if item.executable_with_residual then incr executable_scripts
      end)
    corpus;
  let non_residual_coverage =
    if !total_nodes = 0 then 0.0
    else float_of_int !non_residual_nodes /. float_of_int !total_nodes
  in
  if non_residual_coverage < 0.95 then
    failures :=
      Printf.sprintf
        "non-residual semantic-node coverage is %.2f%%; at least 95%% is \
         required"
        (non_residual_coverage *. 100.0)
      :: !failures;
  if !non_interactive_scripts = 0 then
    failures := "curated corpus has no non-interactive scripts" :: !failures
  else if !executable_scripts <> !non_interactive_scripts then
    failures :=
      Printf.sprintf
        "residual-inclusive execution covers %d/%d non-interactive scripts"
        !executable_scripts !non_interactive_scripts
      :: !failures;
  let expected_embedded = List.sort_uniq String.compare expected_embedded in
  let found_embedded = List.sort_uniq String.compare found_embedded in
  let embedded_found =
    List.fold_left
      (fun count expected ->
        if List.mem expected found_embedded then count + 1 else count)
      0 expected_embedded
  in
  if embedded_found <> List.length expected_embedded then
    failures :=
      Printf.sprintf "embedded-format inventory covers %d/%d known formats"
        embedded_found
        (List.length expected_embedded)
      :: !failures;
  let matrix_passed =
    List.fold_left
      (fun count (operating_system, shell) ->
        if
          List.exists
            (fun entry ->
              entry.operating_system = operating_system
              && entry.shell = shell && entry.passed)
            matrix
        then count + 1
        else count)
      0 required_matrix
  in
  let matrix_required = List.length required_matrix in
  if matrix_passed <> matrix_required then
    failures :=
      Printf.sprintf "release matrix passes %d/%d OS/shell gates" matrix_passed
        matrix_required
      :: !failures;
  let failures = List.rev !failures in
  {
    ready = failures = [];
    total_nodes = !total_nodes;
    non_residual_nodes = !non_residual_nodes;
    non_residual_coverage;
    executable_scripts = !executable_scripts;
    non_interactive_scripts = !non_interactive_scripts;
    embedded_found;
    embedded_expected = List.length expected_embedded;
    matrix_passed;
    matrix_required;
    failures;
  }
