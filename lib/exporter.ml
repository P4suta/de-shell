type target = Internal | Dagger | Nu | Cwl
type artifact = { filename : string; media_type : string; content : string }
type unsupported = { node_id : string; operation : string }

let operation_name (node : Ir.node) =
  match node.operation with
  | Ir.Exec _ -> "exec"
  | Ir.Sequence _ -> "sequence"
  | Ir.Pipeline _ -> "pipeline"
  | Ir.Parallel _ -> "parallel"
  | Ir.Condition _ -> "condition"
  | Ir.Match _ -> "match"
  | Ir.For_each _ -> "foreach"
  | Ir.Try_finally _ -> "try_finally"
  | Ir.Task_call _ -> "task_call"
  | Ir.Set_variable _ -> "set_variable"
  | Ir.Capture_stdout _ -> "capture_stdout"
  | Ir.File_read _ -> "file_read"
  | Ir.File_write _ -> "file_write"
  | Ir.File_remove _ -> "file_remove"
  | Ir.Network_request _ -> "network_request"
  | Ir.Opaque_capsule _ -> "opaque_capsule"

let entry_task plan =
  match
    List.find_opt (fun task -> task.Ir.name = plan.Ir.entrypoint) plan.tasks
  with
  | Some task -> Ok task
  | None -> Error ("entrypoint task not found: " ^ plan.entrypoint)

let unsupported_task_interface (task : Ir.task) =
  if Option.is_some task.invocation then Some "task.invocation"
  else if task.inputs <> [] then Some "task.inputs"
  else if task.outputs <> [] then Some "task.outputs"
  else if task.environment <> [] then Some "task.environment"
  else if task.secrets <> [] then Some "task.secrets"
  else None

let rec flatten_exec node =
  match node.Ir.operation with
  | Ir.Exec command when command.environment <> [] ->
      Error { node_id = node.id; operation = "exec.environment" }
  | Ir.Exec command when command.working_directory <> None ->
      Error { node_id = node.id; operation = "exec.working_directory" }
  | Ir.Exec command -> Ok [ command ]
  | Ir.Sequence nodes ->
      let rec loop accumulator = function
        | [] -> Ok (List.rev accumulator |> List.concat)
        | node :: rest ->
            begin match flatten_exec node with
            | Error _ as error -> error
            | Ok commands -> loop (commands :: accumulator) rest
            end
      in
      loop [] nodes
  | _ -> Error { node_id = node.id; operation = operation_name node }

let json_string value = Yojson.Safe.to_string (`String value)

let json_argv argv =
  Yojson.Safe.to_string (`List (List.map (fun value -> `String value) argv))

let find_substring ~needle value =
  let length = String.length needle in
  let rec loop index =
    if index + length > String.length value then None
    else if String.sub value index length = needle then Some index
    else loop (index + 1)
  in
  if length = 0 then Some 0 else loop 0

let dagger commands =
  let steps =
    commands
    |> List.map (fun command ->
        Printf.sprintf
          "  container = container.withExec(%s);\n\
          \  output += await container.stdout();"
          (json_argv command.Ir.argv))
    |> String.concat "\n"
  in
  {
    filename = "deshell.dagger.ts";
    media_type = "text/typescript";
    content =
      Printf.sprintf
        "import { dag, Container, object, func } from \"@dagger.io/dagger\";\n\n\
         @object()\n\
         export class Deshell {\n\
        \  @func()\n\
        \  async main(): Promise<string> {\n\
        \    let container: Container = \
         dag.container().from(\"alpine@sha256:14358309a308569c32bdc37e2e0e9694be33a9d99e68afb0f5ff33cc1f695dce\");\n\
        \    let output = \"\";\n\
         %s\n\
        \    return output;\n\
        \  }\n\
         }\n"
        steps;
  }

let nu commands =
  let lines =
    commands
    |> List.map (fun command ->
        match command.Ir.argv with
        | [] -> ""
        | executable :: arguments ->
            "  run-external "
            ^ String.concat " " (List.map json_string (executable :: arguments)))
    |> String.concat "\n"
  in
  {
    filename = "deshell.nu";
    media_type = "text/x-nushell";
    content = "export def main [] {\n" ^ lines ^ "\n}\n";
  }

let cwl_command command =
  match command.Ir.argv with
  | [] -> invalid_arg "empty argv"
  | executable :: arguments ->
      let document =
        `Assoc
          [
            ("cwlVersion", `String "v1.2");
            ("class", `String "CommandLineTool");
            ("baseCommand", `List [ `String executable ]);
            ( "arguments",
              `List (List.map (fun value -> `String value) arguments) );
            ("inputs", `Assoc []);
            ( "outputs",
              `Assoc [ ("stdout", `Assoc [ ("type", `String "stdout") ]) ] );
            ("stdout", `String "deshell.stdout");
          ]
      in
      {
        filename = "deshell.cwl";
        media_type = "application/cwl+yaml";
        content = Yojson.Safe.pretty_to_string document ^ "\n";
      }

let balanced source =
  let matching = function '(' -> ')' | '[' -> ']' | '{' -> '}' | _ -> '?' in
  let rec loop index stack quoted escaped =
    if index = String.length source then
      if quoted then Error "unterminated string"
      else if stack <> [] then Error "unclosed delimiter"
      else Ok ()
    else
      let character = source.[index] in
      if quoted then
        if escaped then loop (index + 1) stack true false
        else if character = '\\' then loop (index + 1) stack true true
        else if character = '"' then loop (index + 1) stack false false
        else loop (index + 1) stack true false
      else
        match character with
        | '"' -> loop (index + 1) stack true false
        | '(' | '[' | '{' -> loop (index + 1) (character :: stack) false false
        | ')' | ']' | '}' ->
            begin match stack with
            | opening :: rest when matching opening = character ->
                loop (index + 1) rest false false
            | _ -> Error "mismatched delimiter"
            end
        | _ -> loop (index + 1) stack false false
  in
  loop 0 [] false false

let validate_internal artifact =
  match Ir_codec.decode_string artifact.content with
  | Error errors -> errors
  | Ok plan ->
      begin match Ir.validate_plan plan with
      | Ok () -> []
      | Error errors -> errors
      end

let validate_dagger artifact =
  let errors = ref [] in
  if artifact.filename <> "deshell.dagger.ts" then
    errors := "Dagger artifact filename must be deshell.dagger.ts" :: !errors;
  if artifact.media_type <> "text/typescript" then
    errors := "Dagger artifact media type must be text/typescript" :: !errors;
  let pinned =
    match find_substring ~needle:"alpine@sha256:" artifact.content with
    | None -> false
    | Some index ->
        let digest_start = index + String.length "alpine@sha256:" in
        digest_start + 64 <= String.length artifact.content
        && String.sub artifact.content digest_start 64
           |> String.for_all (function
             | '0' .. '9' | 'a' .. 'f' -> true
             | _ -> false)
  in
  if not pinned then
    errors := "Dagger base container must be digest-pinned" :: !errors;
  begin match balanced artifact.content with
  | Ok () -> ()
  | Error message -> errors := ("invalid TypeScript: " ^ message) :: !errors
  end;
  List.rev !errors

let validate_nu artifact =
  let errors = ref [] in
  if artifact.filename <> "deshell.nu" then
    errors := "Nushell artifact filename must be deshell.nu" :: !errors;
  if not (String.starts_with ~prefix:"export def main [] {" artifact.content)
  then errors := "Nushell module must export main" :: !errors;
  begin match balanced artifact.content with
  | Ok () -> ()
  | Error message -> errors := ("invalid Nushell: " ^ message) :: !errors
  end;
  List.rev !errors

let validate_cwl artifact =
  try
    let document = Yojson.Safe.from_string artifact.content in
    let open Yojson.Safe.Util in
    let errors = ref [] in
    let expect_string name expected =
      match document |> member name with
      | `String value when value = expected -> ()
      | _ ->
          errors := Printf.sprintf "CWL %s must be %s" name expected :: !errors
    in
    expect_string "cwlVersion" "v1.2";
    expect_string "class" "CommandLineTool";
    begin match document |> member "baseCommand" with
    | `List (_ :: _ as values)
      when List.for_all (function `String _ -> true | _ -> false) values ->
        ()
    | _ ->
        errors := "CWL baseCommand must be a non-empty string array" :: !errors
    end;
    begin match document |> member "arguments" with
    | `List values
      when List.for_all (function `String _ -> true | _ -> false) values ->
        ()
    | _ -> errors := "CWL arguments must be a string array" :: !errors
    end;
    begin match document |> member "inputs" with
    | `Assoc _ -> ()
    | _ -> errors := "CWL inputs must be an object" :: !errors
    end;
    begin match document |> member "outputs" with
    | `Assoc outputs ->
        begin match List.assoc_opt "stdout" outputs with
        | Some (`Assoc fields) ->
            begin match List.assoc_opt "type" fields with
            | Some (`String "stdout") -> ()
            | _ ->
                errors := "CWL stdout output must have type stdout" :: !errors
            end
        | _ -> errors := "CWL outputs.stdout must be an object" :: !errors
        end
    | _ -> errors := "CWL outputs must be an object" :: !errors
    end;
    List.rev !errors
  with
  | Yojson.Json_error message -> [ "invalid CWL JSON: " ^ message ]
  | Yojson.Safe.Util.Type_error (message, _) ->
      [ "invalid CWL structure: " ^ message ]

let validate_artifact ~target artifact =
  let errors =
    match target with
    | Internal -> validate_internal artifact
    | Dagger -> validate_dagger artifact
    | Nu -> validate_nu artifact
    | Cwl -> validate_cwl artifact
  in
  if errors = [] then Ok () else Error errors

let validated target artifact =
  match validate_artifact ~target artifact with
  | Ok () -> Ok artifact
  | Error errors ->
      Error
        ("exporter produced an invalid artifact: " ^ String.concat "; " errors)

let bridge_artifact target =
  let argv = [ "deshell"; "run"; "--allow-residual" ] in
  match target with
  | Internal ->
      { filename = "plan.json"; media_type = "application/json"; content = "" }
  | Dagger -> dagger [ Ir.exec argv ]
  | Nu -> nu [ Ir.exec argv ]
  | Cwl -> cwl_command (Ir.exec argv)

let export ~target ~bridge plan =
  match target with
  | Internal ->
      validated Internal
        {
          filename = "plan.json";
          media_type = "application/vnd.deshell.effect-ir+json";
          content = Ir_codec.encode_string plan;
        }
  | Dagger | Nu | Cwl ->
      begin match entry_task plan with
      | Error _ as error -> error
      | Ok task ->
          begin match unsupported_task_interface task with
          | Some operation ->
              Error
                (Printf.sprintf
                   "strict exporter cannot represent task %s (%s); bridge \
                    cannot preserve this task interface yet"
                   task.name operation)
          | None ->
              begin match flatten_exec task.body with
              | Error _ when bridge -> validated target (bridge_artifact target)
              | Error unsupported ->
                  Error
                    (Printf.sprintf
                       "strict exporter cannot represent node %s (%s); use \
                        --bridge explicitly"
                       unsupported.node_id unsupported.operation)
              | Ok commands -> (
                  match target with
                  | Dagger -> validated target (dagger commands)
                  | Nu -> validated target (nu commands)
                  | Cwl ->
                      begin match commands with
                      | [ command ] -> validated target (cwl_command command)
                      | _ ->
                          Error
                            "strict CWL CommandLineTool export requires \
                             exactly one Exec node"
                      end
                  | Internal -> assert false)
              end
          end
      end
