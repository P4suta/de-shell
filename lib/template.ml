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

let rec collect ~bound names (node : Ir.node) =
  let collect_node = collect ~bound in
  match node.operation with
  | Ir.Exec command ->
      let names = add_texts ~bound names command.argv in
      let names =
        List.fold_left
          (fun names (_, value) -> add_text ~bound names value)
          names command.environment
      in
      Option.fold ~none:names
        ~some:(fun value -> add_text ~bound names value)
        command.working_directory
  | Ir.Pipeline nodes | Ir.Sequence nodes | Ir.Parallel nodes ->
      List.fold_left collect_node names nodes
  | Ir.Condition { predicate; if_true; if_false } ->
      let names =
        collect_node names predicate |> fun names -> collect_node names if_true
      in
      Option.fold ~none:names ~some:(collect_node names) if_false
  | Ir.Match { value; cases; default } ->
      let names = add_text ~bound names value in
      let names =
        List.fold_left
          (fun names (_, branch) -> collect_node names branch)
          names cases
      in
      Option.fold ~none:names ~some:(collect_node names) default
  | Ir.For_each { variable; items; body } ->
      let names = add_texts ~bound names items in
      collect ~bound:(Names.add variable bound) names body
  | Ir.Try_finally { body; finalizer } ->
      collect_node (collect_node names body) finalizer
  | Ir.Task_call call ->
      List.fold_left
        (fun names (_, value) -> add_text ~bound names value)
        names call.arguments
  | Ir.File_read path | Ir.File_remove path -> add_text ~bound names path
  | Ir.File_write write ->
      add_text ~bound (add_text ~bound names write.path) write.contents
  | Ir.Network_request request ->
      add_text ~bound (add_text ~bound names request.method_) request.uri
  | Ir.Opaque_capsule _ -> names

let environment_variables node =
  collect ~bound:Names.empty Names.empty node |> Names.elements
