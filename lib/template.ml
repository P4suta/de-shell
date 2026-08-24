module Names = Set.Make (String)

let valid_name value =
  value <> ""
  &&
  match value.[0] with
  | 'A' .. 'Z' | 'a' .. 'z' | '_' ->
      String.for_all
        (function
          | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> true | _ -> false)
        value
  | _ -> false

let variables_in_text text =
  let length = String.length text in
  let rec find_close index =
    if index >= length then None
    else if text.[index] = '}' then Some index
    else find_close (index + 1)
  in
  let rec loop index names =
    if index >= length then names
    else if index + 1 < length && text.[index] = '$' && text.[index + 1] = '$'
    then loop (index + 2) names
    else if index + 1 < length && text.[index] = '$' && text.[index + 1] = '{'
    then
      match find_close (index + 2) with
      | None -> names
      | Some close ->
          let expression = String.sub text (index + 2) (close - index - 2) in
          let name =
            match String.index_opt expression ':' with
            | Some separator
              when separator + 1 < String.length expression
                   && expression.[separator + 1] = '-' ->
                String.sub expression 0 separator
            | Some _ -> ""
            | None -> expression
          in
          let names = if valid_name name then Names.add name names else names in
          loop (close + 1) names
    else loop (index + 1) names
  in
  loop 0 Names.empty

let add_text ~bound names value =
  Names.union names (Names.diff (variables_in_text value) bound)

let add_texts ~bound names values =
  List.fold_left (add_text ~bound) names values

let intersect_bounds = function
  | [] -> Names.empty
  | first :: rest -> List.fold_left Names.inter first rest

let rec collect ~bound names (node : Ir.node) =
  let collect_isolated names nodes =
    List.fold_left
      (fun names node -> collect ~bound names node |> fst)
      names nodes
  in
  match node.operation with
  | Ir.Exec command ->
      let names = add_texts ~bound names command.argv in
      let names =
        List.fold_left
          (fun names (_, value) -> add_text ~bound names value)
          names command.environment
      in
      let names =
        Option.fold ~none:names
          ~some:(fun value -> add_text ~bound names value)
          command.working_directory
      in
      (names, bound)
  | Ir.Pipeline nodes | Ir.Parallel nodes ->
      (collect_isolated names nodes, bound)
  | Ir.Sequence nodes ->
      List.fold_left
        (fun (names, bound) node -> collect ~bound names node)
        (names, bound) nodes
  | Ir.Condition { predicate; if_true; if_false } ->
      let names, predicate_bound = collect ~bound names predicate in
      let names, true_bound = collect ~bound:predicate_bound names if_true in
      let names, false_bound =
        match if_false with
        | None -> (names, predicate_bound)
        | Some branch -> collect ~bound:predicate_bound names branch
      in
      (names, Names.inter true_bound false_bound)
  | Ir.Match { value; cases; default } ->
      let names = add_text ~bound names value in
      let names, branch_bounds =
        List.fold_left
          (fun (names, bounds) (_, branch) ->
            let names, branch_bound = collect ~bound names branch in
            (names, branch_bound :: bounds))
          (names, []) cases
      in
      let names, branch_bounds =
        match default with
        | Some branch ->
            let names, branch_bound = collect ~bound names branch in
            (names, branch_bound :: branch_bounds)
        | None -> (names, bound :: branch_bounds)
      in
      (names, intersect_bounds branch_bounds)
  | Ir.For_each { variable; items; body } ->
      let names = add_texts ~bound names items in
      let names, _ = collect ~bound:(Names.add variable bound) names body in
      (names, bound)
  | Ir.Try_finally { body; finalizer } ->
      let names, body_bound = collect ~bound names body in
      collect ~bound:body_bound names finalizer
  | Ir.Task_call call ->
      ( List.fold_left
          (fun names (_, value) -> add_text ~bound names value)
          names call.arguments,
        bound )
  | Ir.Set_variable assignment ->
      (add_text ~bound names assignment.value, Names.add assignment.name bound)
  | Ir.Capture_stdout capture ->
      let names, _ = collect ~bound names capture.body in
      (names, Names.add capture.name bound)
  | Ir.File_read path | Ir.File_remove path ->
      (add_text ~bound names path, bound)
  | Ir.File_write write ->
      (add_text ~bound (add_text ~bound names write.path) write.contents, bound)
  | Ir.Network_request request ->
      ( add_text ~bound (add_text ~bound names request.method_) request.uri,
        bound )
  | Ir.Opaque_capsule _ -> (names, bound)

let environment_variables node =
  collect ~bound:Names.empty Names.empty node |> fst |> Names.elements
