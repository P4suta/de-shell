type candidate = { scenario : Scenario.t; rationale : string }

type result = {
  environment_variables : string list;
  arguments : int list;
  candidates : candidate list;
}

type environment_reference = {
  name : string;
  default : string option;
  branch : string option;
}

type reference = Argument of int | Environment of environment_reference

let identifier_start = function
  | 'A' .. 'Z' | 'a' .. 'z' | '_' -> true
  | _ -> false

let identifier_character = function
  | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' -> true
  | _ -> false

let branch_value source index =
  let length = String.length source in
  let rec skip index =
    if
      index < length
      && (source.[index] = ' ' || source.[index] = '\t' || source.[index] = '"')
    then skip (index + 1)
    else index
  in
  let index = skip index in
  if index >= length || source.[index] <> '=' then None
  else
    let index =
      if index + 1 < length && source.[index + 1] = '=' then index + 2
      else index + 1
    in
    let index = skip index in
    let rec finish cursor =
      if cursor < length then
        match source.[cursor] with
        | 'A' .. 'Z' | 'a' .. 'z' | '0' .. '9' | '_' | '-' | '.' | '/' ->
            finish (cursor + 1)
        | _ -> cursor
      else cursor
    in
    let ending = finish index in
    if ending = index then None
    else Some (String.sub source index (ending - index))

let braced_reference source start close =
  let body = String.sub source start (close - start) in
  let name_end =
    let rec loop index =
      if index < String.length body && identifier_character body.[index] then
        loop (index + 1)
      else index
    in
    loop 0
  in
  if name_end = 0 then None
  else
    let name = String.sub body 0 name_end in
    let default =
      if name_end + 2 <= String.length body && String.sub body name_end 2 = ":-"
      then
        Some
          (String.sub body (name_end + 2) (String.length body - name_end - 2))
      else None
    in
    Some (name, default)

let references source =
  let length = String.length source in
  let values = ref [] in
  let state = ref `Normal in
  let index = ref 0 in
  let add_reference ending reference =
    let reference =
      match reference with
      | Environment value ->
          Environment { value with branch = branch_value source ending }
      | Argument _ as value -> value
    in
    values := reference :: !values
  in
  while !index < length do
    let character = source.[!index] in
    begin match !state with
    | `Single -> if character = '\'' then state := `Normal
    | `Double ->
        if character = '"' then state := `Normal
        else if character = '\\' && !index + 1 < length then incr index
        else if character = '$' then
          begin if !index + 1 < length then
            match source.[!index + 1] with
            | '1' .. '9' as digit ->
                add_reference (!index + 2)
                  (Argument (Char.code digit - Char.code '0'));
                incr index
            | '{' ->
                begin match String.index_from_opt source (!index + 2) '}' with
                | None -> ()
                | Some close ->
                    begin match braced_reference source (!index + 2) close with
                    | None -> ()
                    | Some (name, default) ->
                        add_reference (close + 1)
                          (Environment { name; default; branch = None })
                    end;
                    index := close
                end
            | first when identifier_start first ->
                let rec finish cursor =
                  if cursor < length && identifier_character source.[cursor]
                  then finish (cursor + 1)
                  else cursor
                in
                let ending = finish (!index + 1) in
                let name =
                  String.sub source (!index + 1) (ending - !index - 1)
                in
                add_reference ending
                  (Environment { name; default = None; branch = None });
                index := ending - 1
            | _ -> ()
          end
    | `Normal ->
        begin match character with
        | '\'' -> state := `Single
        | '"' -> state := `Double
        | '#' ->
            while !index < length && source.[!index] <> '\n' do
              incr index
            done;
            decr index
        | '\\' when !index + 1 < length -> incr index
        | '$' ->
            if !index + 1 < length then
              begin match source.[!index + 1] with
              | '1' .. '9' as digit ->
                  add_reference (!index + 2)
                    (Argument (Char.code digit - Char.code '0'));
                  incr index
              | '{' ->
                  begin match String.index_from_opt source (!index + 2) '}' with
                  | None -> ()
                  | Some close ->
                      begin match
                        braced_reference source (!index + 2) close
                      with
                      | None -> ()
                      | Some (name, default) ->
                          add_reference (close + 1)
                            (Environment { name; default; branch = None })
                      end;
                      index := close
                  end
              | first when identifier_start first ->
                  let rec finish cursor =
                    if cursor < length && identifier_character source.[cursor]
                    then finish (cursor + 1)
                    else cursor
                  in
                  let ending = finish (!index + 1) in
                  let name =
                    String.sub source (!index + 1) (ending - !index - 1)
                  in
                  add_reference ending
                    (Environment { name; default = None; branch = None });
                  index := ending - 1
              | _ -> ()
              end
        | _ -> ()
        end
    end;
    incr index
  done;
  List.rev !values

let contains_ci ~needle value =
  let needle = String.uppercase_ascii needle in
  let value = String.uppercase_ascii value in
  let needle_length = String.length needle in
  let rec loop index =
    index + needle_length <= String.length value
    && (String.sub value index needle_length = needle || loop (index + 1))
  in
  needle_length = 0 || loop 0

let secret_name name =
  List.exists
    (fun marker -> contains_ci ~needle:marker name)
    [ "TOKEN"; "PASSWORD"; "SECRET"; "PRIVATE_KEY"; "API_KEY" ]

let empty_expectation =
  Scenario.{ exit_code = None; stdout = None; stderr = None; files = [] }

let scenario name args environment =
  Scenario.
    {
      name;
      args;
      environment = List.sort compare environment;
      fixtures = [];
      timeout_ms = 30000;
      expect = empty_expectation;
    }

let sanitize_name value =
  value
  |> String.map (function
    | 'A' .. 'Z' as character -> Char.lowercase_ascii character
    | ('a' .. 'z' | '0' .. '9' | '-' | '_') as character -> character
    | _ -> '-')

let take count values =
  let rec loop remaining accumulator = function
    | _ when remaining = 0 -> List.rev accumulator
    | [] -> List.rev accumulator
    | value :: rest -> loop (remaining - 1) (value :: accumulator) rest
  in
  loop count [] values

let suggest ~max_scenarios ~source =
  if max_scenarios <= 0 then invalid_arg "max_scenarios must be positive";
  let references = references source in
  let arguments =
    references
    |> List.filter_map (function
      | Argument index -> Some index
      | Environment _ -> None)
    |> List.sort_uniq compare
  in
  let environment_references =
    references
    |> List.filter_map (function
      | Environment value -> Some value
      | Argument _ -> None)
  in
  let environment_variables =
    environment_references
    |> List.map (fun value -> value.name)
    |> List.sort_uniq String.compare
  in
  let candidates =
    ref
      [
        {
          scenario = scenario "concolic-default" [] [];
          rationale = "all discovered inputs absent";
        };
      ]
  in
  List.iter
    (fun index ->
      let args =
        List.init index (fun position ->
            Printf.sprintf "deshell-arg-%d" (position + 1))
      in
      candidates :=
        {
          scenario = scenario (Printf.sprintf "concolic-arg-%d" index) args [];
          rationale =
            Printf.sprintf "positional argument $%d is populated" index;
        }
        :: !candidates)
    arguments;
  List.iter
    (fun name ->
      let references =
        List.filter
          (fun reference -> reference.name = name)
          environment_references
      in
      let values =
        if secret_name name then [ "<secret:" ^ name ^ ">" ]
        else
          List.concat_map
            (fun reference ->
              Option.to_list reference.branch @ Option.to_list reference.default)
            references
          @ [ "deshell-" ^ name ]
          |> List.sort_uniq String.compare
      in
      List.iter
        (fun value ->
          candidates :=
            {
              scenario =
                scenario
                  ("concolic-env-" ^ sanitize_name name ^ "-"
                  ^ String.sub (Sha256.hex value) 0 8)
                  []
                  [ (name, value) ];
              rationale = name ^ " is populated with " ^ value;
            }
            :: !candidates)
        values)
    environment_variables;
  let candidates =
    List.rev !candidates
    |> List.sort_uniq (fun left right ->
        compare
          (left.scenario.args, left.scenario.environment)
          (right.scenario.args, right.scenario.environment))
    |> take max_scenarios
  in
  { environment_variables; arguments; candidates }
