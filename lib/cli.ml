open Cmdliner

type output_format = Human | Json
type target = Internal | Dagger | Nu | Cwl

let version = "0.1.0"

let root_argument =
  Arg.(
    value & opt string "."
    & info [ "root" ] ~docv:"DIR" ~doc:"Project root directory.")

let entry_argument =
  Arg.(
    value
    & opt (some string) None
    & info [ "entry" ] ~docv:"PATH"
        ~doc:
          "Entrypoint path relative to the project root. Uses the sole \
           configured entrypoint when omitted.")

let select_entry root = function
  | Some entry -> Ok entry
  | None -> Project.configured_entry ~root

let apply_argument =
  Arg.(
    value & flag & info [ "apply" ] ~doc:"Apply the preview transactionally.")

let init root =
  match Project.init ~root with
  | Error message -> Error message
  | Ok initialized ->
      if initialized.created = [] then
        Printf.printf "de-shell already initialized in %s\n" root
      else begin
        Printf.printf "initialized de-shell in %s\n" root;
        List.iter
          (fun path -> Printf.printf "created %s\n" path)
          initialized.created
      end;
      Ok 0

let init_command =
  let info =
    Cmd.info "init" ~doc:"Initialize canonical de-shell project files."
  in
  Cmd.make info Term.(const init $ root_argument)

let output_format_argument =
  let values = Arg.enum [ ("human", Human); ("json", Json) ] in
  Arg.(
    value & opt values Human
    & info [ "format" ] ~docv:"FORMAT" ~doc:"Output format: human or json.")

let scan root format =
  match Project.scan ~root with
  | Error message -> Error message
  | Ok findings ->
      begin match format with
      | Json ->
          findings |> List.map Scanner.to_yojson |> fun values ->
          Yojson.Safe.pretty_to_string (`List values) |> print_endline
      | Human ->
          List.iter
            (fun (finding : Scanner.finding) ->
              Printf.printf "%s\t%s%s\n"
                (Scanner.kind_to_string finding.kind)
                finding.path
                (Option.fold ~none:""
                   ~some:(fun locator -> "#" ^ locator)
                   finding.locator))
            findings;
          Printf.printf "%d shell location(s) found\n" (List.length findings)
      end;
      Ok 0

let scan_command =
  let info =
    Cmd.info "scan"
      ~doc:"Inventory shell files, embedded shell, and candidates."
  in
  Cmd.make info Term.(const scan $ root_argument $ output_format_argument)

let analyze root requested_entry =
  match select_entry root requested_entry with
  | Error message -> Error message
  | Ok entry -> (
      match Project.analyze ~root ~entry with
      | Error message -> Error message
      | Ok result ->
          Printf.printf "wrote %s\nwrote %s\n" result.plan_path
            result.evidence_path;
          Ok 0)

let analyze_command =
  let info =
    Cmd.info "analyze" ~doc:"Lower an entrypoint into canonical Effect IR."
  in
  Cmd.make info Term.(const analyze $ root_argument $ entry_argument)

let equivalent_argument =
  Arg.(
    value & flag
    & info [ "equivalent" ]
        ~doc:"Restrict rewriting to meaning-preserving rules.")

let rewrite root requested_entry equivalent apply =
  if not equivalent then Error "rewrite requires --equivalent"
  else
    match select_entry root requested_entry with
    | Error message -> Error message
    | Ok entry -> (
        match Project.rewrite_equivalent ~root ~entry ~apply with
        | Error message -> Error message
        | Ok result ->
            if not result.changed then
              Printf.printf "%s: no equivalent rewrite available\n" entry
            else if result.applied then
              Printf.printf "%s: applied %d equivalent edit(s)\n" entry
                (List.length result.edits)
            else print_string result.preview;
            Ok 0)

let rewrite_command =
  let info =
    Cmd.info "rewrite" ~doc:"Preview or apply equivalent shell rewrites."
  in
  Cmd.make info
    Term.(
      const rewrite $ root_argument $ entry_argument $ equivalent_argument
      $ apply_argument)

let profile_argument =
  Arg.(
    required
    & opt (some string) None
    & info [ "profile" ] ~docv:"PROFILES"
        ~doc:
          "Comma-separated modernization profiles: portable, secure, \
           reproducible.")

let modernize root profiles apply =
  let profile_names =
    String.split_on_char ',' profiles |> List.map String.trim
  in
  let unknown =
    List.filter
      (fun profile ->
        not (List.mem profile [ "portable"; "secure"; "reproducible" ]))
      profile_names
  in
  if unknown <> [] then
    Error ("unknown modernization profile: " ^ String.concat ", " unknown)
  else
    match Project.scan ~root with
    | Error message -> Error message
    | Ok findings ->
        let profiles =
          List.map
            (function
              | "portable" -> Modernizer.Portable
              | "secure" -> Modernizer.Secure
              | "reproducible" -> Modernizer.Reproducible
              | _ -> assert false)
            profile_names
        in
        let paths =
          findings
          |> List.filter_map (fun (finding : Scanner.finding) ->
              if finding.kind = Scanner.Shell_file then Some finding.path
              else None)
          |> List.sort_uniq String.compare
        in
        let rec prepare changes = function
          | [] -> Ok (List.rev changes)
          | path :: rest ->
              begin match Project.resolve_entry ~root path with
              | Error message -> Error message
              | Ok (_, absolute) ->
                  begin try
                    let source = Project.read_file absolute in
                    let proposal =
                      Atomic_patch.prepare ~path:absolute ~replacement:source
                    in
                    let modernization =
                      Modernizer.propose ~path ~profiles source
                    in
                    List.iter
                      (fun (finding : Modernizer.finding) ->
                        Printf.printf "%s: %s: %s\n" path finding.rule
                          finding.message)
                      modernization.findings;
                    if modernization.output = source then prepare changes rest
                    else
                      let proposal =
                        { proposal with replacement = modernization.output }
                      in
                      let preview =
                        Project.simple_preview ~path ~before:source
                          ~after:modernization.output
                      in
                      prepare
                        (( path,
                           proposal,
                           preview,
                           List.length modernization.edits )
                        :: changes)
                        rest
                  with Sys_error message -> Error message
                  end
              end
        in
        begin match prepare [] paths with
        | Error _ as error -> error
        | Ok [] ->
            Printf.printf "modernization proposal: no applicable changes\n";
            Ok 0
        | Ok changes when not apply ->
            List.iter (fun (_, _, preview, _) -> print_string preview) changes;
            Ok 0
        | Ok changes ->
            let proposals =
              List.map (fun (_, proposal, _, _) -> proposal) changes
            in
            begin match Atomic_patch.apply_all proposals with
            | Error message -> Error message
            | Ok () ->
                List.iter
                  (fun (path, _, _, edit_count) ->
                    Printf.printf "%s: applied %d modernization edit(s)\n" path
                      edit_count)
                  changes;
                Ok 0
            end
        end

let modernize_command =
  let info =
    Cmd.info "modernize"
      ~doc:"Propose explicitly behavior-changing improvements."
  in
  Cmd.make info
    Term.(const modernize $ root_argument $ profile_argument $ apply_argument)

let observe_argument =
  Arg.(
    value & flag
    & info [ "observe" ] ~doc:"Request disposable observation before lowering.")

let target_argument =
  let targets =
    Arg.enum
      [ ("internal", Internal); ("dagger", Dagger); ("nu", Nu); ("cwl", Cwl) ]
  in
  Arg.(
    required
    & opt (some targets) None
    & info [ "target" ] ~docv:"TARGET" ~doc:"Migration/export target.")

let migrate root requested_entry observe target apply =
  match select_entry root requested_entry with
  | Error message -> Error message
  | Ok entry -> (
      match Project.analyze ~root ~entry with
      | Error message -> Error message
      | Ok result ->
          let observation_result =
            if not observe then Ok None
            else
              match Observation_run.run ~root ~entry ~plan:result.plan with
              | Error message -> Error message
              | Ok outcome ->
                  begin match
                    Project.record_observation ~root
                      (Observation_run.result_to_yojson outcome)
                  with
                  | Error message -> Error message
                  | Ok () ->
                      Printf.eprintf "observation: %s%s\n"
                        (Observation_run.status_to_string outcome.status)
                        (Option.fold ~none:""
                           ~some:(fun reason -> ": " ^ reason)
                           outcome.reason);
                      if Observation_run.blocks_migration outcome then
                        Error
                          "migration stopped because observed behavior differs"
                      else Ok (Some outcome)
                  end
          in
          begin match observation_result with
          | Error message -> Error message
          | Ok _ ->
              let exporter_target =
                match target with
                | Internal -> Exporter.Internal
                | Dagger -> Exporter.Dagger
                | Nu -> Exporter.Nu
                | Cwl -> Exporter.Cwl
              in
              begin match
                Exporter.export ~target:exporter_target ~bridge:false
                  result.plan
              with
              | Error message -> Error message
              | Ok artifact ->
                  let exported_artifact =
                    if target = Internal then None else Some artifact
                  in
                  begin match
                    Migration.prepare ~root ~entry ~artifact:exported_artifact
                  with
                  | Error message -> Error message
                  | Ok migration ->
                      if apply then
                        begin match Migration.apply migration with
                        | Error message -> Error message
                        | Ok () ->
                            Option.iter
                              (fun path -> Printf.printf "wrote %s\n" path)
                              migration.artifact_path;
                            List.iter
                              (fun path -> Printf.printf "patched %s\n" path)
                              migration.caller_files;
                            if target = Internal then
                              Printf.printf "plan: %s\n" result.plan_path;
                            Ok 0
                        end
                      else begin
                        if target = Internal then
                          Printf.printf "plan: %s\n" result.plan_path;
                        print_string migration.preview;
                        Ok 0
                      end
                  end
              end
          end)

let migrate_command =
  let info =
    Cmd.info "migrate" ~doc:"Observe, lower, and migrate an entrypoint."
  in
  Cmd.make info
    Term.(
      const migrate $ root_argument $ entry_argument $ observe_argument
      $ target_argument $ apply_argument)

let check root =
  match Project.check ~root with
  | Error message -> Error message
  | Ok () ->
      Printf.printf "%s: project artifacts are valid\n" root;
      Ok 0

let check_command =
  let info =
    Cmd.info "check" ~doc:"Validate config, lock, plan, and evidence artifacts."
  in
  Cmd.make info Term.(const check $ root_argument)

let verify root =
  match Project.load_plan ~root with
  | Error message -> Error message
  | Ok plan ->
      begin match Verifier.audit plan with
      | Error errors -> Error (String.concat "; " errors)
      | Ok report ->
          Printf.printf "formal=%d exhaustive=%d differential=%d residual=%d\n"
            report.formal report.exhaustive report.differential report.residual;
          List.iter
            (fun reason -> Printf.printf "residual: %s\n" reason)
            report.residual_reasons;
          Ok 0
      end

let verify_command =
  Cmd.make
    (Cmd.info "verify"
       ~doc:
         "Audit guarantee coverage; differential evidence is reported only \
          when an observer recorded it.")
    Term.(const verify $ root_argument)

let allow_residual_argument =
  Arg.(
    value & flag
    & info [ "allow-residual" ]
        ~doc:
          "Allow execution of opaque capsules through their pinned interpreter.")

let allow_file_read_argument =
  Arg.(
    value & flag
    & info [ "allow-file-read" ] ~doc:"Allow project-scoped file reads.")

let allow_file_write_argument =
  Arg.(
    value & flag
    & info [ "allow-file-write" ]
        ~doc:"Allow project-scoped file writes/removals.")

let allow_network_argument =
  Arg.(
    value & flag
    & info [ "allow-network" ]
        ~doc:
          "Allow network nodes (requires a configured record/replay backend).")

let script_arguments_argument =
  let options =
    Arg.(
      value & opt_all string []
      & info [ "arg" ] ~docv:"VALUE"
          ~doc:
            "Pass one original script argument. Use --arg=VALUE when VALUE \
             begins with a dash.")
  in
  let positional =
    Arg.(
      value & pos_all string []
      & info [] ~docv:"SCRIPT_ARG"
          ~doc:"Arguments after -- are passed to the original script.")
  in
  Term.(
    const (fun options positional -> options @ positional)
    $ options $ positional)

let run_node_argument =
  Arg.(value & opt (some string) None & info [ "node" ] ~docv:"NODE_ID")

let rec clone_node_ids ~prefix (node : Ir.node) =
  let clone = clone_node_ids ~prefix in
  let operation =
    match node.operation with
    | Ir.Exec command -> Ir.Exec command
    | Ir.Pipeline nodes -> Ir.Pipeline (List.map clone nodes)
    | Ir.Sequence nodes -> Ir.Sequence (List.map clone nodes)
    | Ir.Parallel nodes -> Ir.Parallel (List.map clone nodes)
    | Ir.Condition { predicate; if_true; if_false } ->
        Ir.Condition
          {
            predicate = clone predicate;
            if_true = clone if_true;
            if_false = Option.map clone if_false;
          }
    | Ir.Match { value; cases; default } ->
        Ir.Match
          {
            value;
            cases =
              List.map (fun (pattern, body) -> (pattern, clone body)) cases;
            default = Option.map clone default;
          }
    | Ir.For_each { variable; items; body } ->
        Ir.For_each { variable; items; body = clone body }
    | Ir.Try_finally { body; finalizer } ->
        Ir.Try_finally { body = clone body; finalizer = clone finalizer }
    | Ir.Task_call call -> Ir.Task_call call
    | Ir.File_read path -> Ir.File_read path
    | Ir.File_write write -> Ir.File_write write
    | Ir.File_remove path -> Ir.File_remove path
    | Ir.Network_request request -> Ir.Network_request request
    | Ir.Opaque_capsule capsule -> Ir.Opaque_capsule capsule
  in
  { node with id = prefix ^ node.id; operation }

let select_node plan node_id =
  match node_id with
  | None -> Ok plan
  | Some id ->
      let found =
        List.fold_left
          (fun result task ->
            match result with
            | Some _ -> result
            | None ->
                Ir.fold_nodes
                  (fun found node ->
                    match found with
                    | Some _ -> found
                    | None ->
                        if node.Ir.id = id then Some (task, node) else None)
                  None task.Ir.body)
          None plan.Ir.tasks
      in
      begin match found with
      | None -> Error ("node not found: " ^ id)
      | Some (owner, body) ->
          let task_names = List.map (fun task -> task.Ir.name) plan.Ir.tasks in
          let rec fresh_task_name index =
            let candidate = Printf.sprintf "__deshell_selected_%d" index in
            if List.mem candidate task_names then fresh_task_name (index + 1)
            else candidate
          in
          let selected_task = fresh_task_name 0 in
          let existing_ids = Hashtbl.create 32 in
          List.iter
            (fun task ->
              Ir.fold_nodes
                (fun () node -> Hashtbl.replace existing_ids node.Ir.id ())
                () task.Ir.body)
            plan.tasks;
          let rec fresh_prefix index =
            let prefix = Printf.sprintf "__selected_%d:" index in
            let conflict =
              Ir.fold_nodes
                (fun conflict node ->
                  conflict || Hashtbl.mem existing_ids (prefix ^ node.Ir.id))
                false body
            in
            if conflict then fresh_prefix (index + 1) else prefix
          in
          let selected_body = clone_node_ids ~prefix:(fresh_prefix 0) body in
          let wrapper =
            Ir.task ~name:selected_task ~inputs:owner.inputs
              ~outputs:owner.outputs ~environment:owner.environment
              ~secrets:owner.secrets
              ~platform_capabilities:owner.platform_capabilities
              ~cacheable:false ?invocation:owner.invocation ~body:selected_body
              ()
          in
          Ok
            {
              plan with
              entrypoint = selected_task;
              tasks = wrapper :: plan.tasks;
            }
      end

let run root node_id allow_residual allow_file_read allow_file_write
    allow_network arguments =
  match Project.load_plan ~root with
  | Error message -> Error message
  | Ok plan ->
      begin match select_node plan node_id with
      | Error message -> Error message
      | Ok plan ->
          let policy : Runner.policy =
            {
              allow_file_read;
              allow_file_write;
              allow_network;
              allow_opaque = allow_residual;
            }
          in
          begin try
            let backend = Process_backend.create ~root in
            let inputs =
              plan.tasks
              |> List.concat_map (fun task -> task.Ir.environment)
              |> List.sort_uniq String.compare
              |> List.filter_map (fun name ->
                  Option.map (fun value -> (name, value)) (Sys.getenv_opt name))
            in
            match
              Runner.run_plan_with_inputs ~backend ~policy ~inputs ~arguments
                plan
            with
            | Error message -> Error message
            | Ok observation ->
                print_string observation.stdout;
                prerr_string observation.stderr;
                Ok observation.exit_code
          with
          | Sys_error message -> Error message
          | Unix.Unix_error (error, function_name, argument) ->
              Error
                (Printf.sprintf "%s(%s): %s" function_name argument
                   (Unix.error_message error))
          end
      end

let run_command =
  Cmd.make
    (Cmd.info "run" ~doc:"Run the canonical Effect IR plan.")
    Term.(
      const run $ root_argument $ run_node_argument $ allow_residual_argument
      $ allow_file_read_argument $ allow_file_write_argument
      $ allow_network_argument $ script_arguments_argument)

let bridge_argument =
  Arg.(
    value & flag
    & info [ "bridge" ] ~doc:"Allow delegation to the internal runner.")

let output_argument =
  Arg.(value & opt (some string) None & info [ "output"; "o" ] ~docv:"FILE")

let export root target bridge output =
  match Project.load_plan ~root with
  | Error message -> Error message
  | Ok plan ->
      let exporter_target =
        match target with
        | Internal -> Exporter.Internal
        | Dagger -> Exporter.Dagger
        | Nu -> Exporter.Nu
        | Cwl -> Exporter.Cwl
      in
      begin match Exporter.export ~target:exporter_target ~bridge plan with
      | Error message -> Error message
      | Ok artifact ->
          begin match output with
          | None -> print_string artifact.content
          | Some path ->
              let path =
                if Filename.is_relative path then Filename.concat root path
                else path
              in
              Project.write_file path artifact.content;
              Printf.printf "wrote %s\n" path
          end;
          Ok 0
      end

let export_command =
  Cmd.make
    (Cmd.info "export"
       ~doc:"Export the canonical plan without dropping effects.")
    Term.(
      const export $ root_argument $ target_argument $ bridge_argument
      $ output_argument)

let node_argument =
  Arg.(value & pos 0 (some string) None & info [] ~docv:"NODE_ID")

let explain root node_id =
  let plan_path =
    Filename.concat (Filename.concat root ".deshell") "plan.json"
  in
  try
    let plan =
      match Ir_codec.decode_string (Project.read_file plan_path) with
      | Ok plan -> plan
      | Error errors -> failwith (String.concat "; " errors)
    in
    let nodes =
      List.fold_left
        (fun values task ->
          Ir.fold_nodes (fun items node -> node :: items) values task.Ir.body)
        [] plan.Ir.tasks
    in
    begin match node_id with
    | None ->
        Printf.printf "entrypoint: %s\ntasks: %d\nnodes: %d\n" plan.entrypoint
          (List.length plan.tasks) (List.length nodes)
    | Some id ->
        begin match List.find_opt (fun node -> node.Ir.id = id) nodes with
        | None -> failwith ("node not found: " ^ id)
        | Some node ->
            Printf.printf "%s\n%s\n" node.id
              (Yojson.Safe.pretty_to_string
                 (Ir_codec.encode_guarantee node.guarantee))
        end
    end;
    Ok 0
  with Failure message | Sys_error message -> Error message

let explain_command =
  Cmd.make
    (Cmd.info "explain" ~doc:"Explain a plan or an individual guarantee.")
    Term.(const explain $ root_argument $ node_argument)

let info =
  Cmd.info "deshell" ~version
    ~doc:
      "Compile shell automation behavior into typed, evidence-carrying Effect \
       IR."

let command =
  Cmd.group info
    [
      init_command;
      scan_command;
      analyze_command;
      rewrite_command;
      modernize_command;
      migrate_command;
      verify_command;
      run_command;
      export_command;
      check_command;
      explain_command;
    ]

let main () = Cmd.eval_result' command
