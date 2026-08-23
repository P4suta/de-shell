type status = Verified | Different | Unavailable | Failed

type outcome = {
  status : status;
  provider : string option;
  reason : string option;
  report : Differential.report option;
}

let status_to_string = function
  | Verified -> "verified"
  | Different -> "different"
  | Unavailable -> "unavailable"
  | Failed -> "failed"

let provider_of_string = function
  | "podman" -> Ok Lab.Podman
  | "docker-rootless" | "docker" -> Ok Lab.Docker_rootless
  | "windows-sandbox" -> Ok Lab.Windows_sandbox
  | "hyper-v" -> Ok Lab.Hyper_v
  | "virtualization-framework" | "vz" -> Ok Lab.Virtualization_framework
  | value -> Error ("unknown observer provider: " ^ value)

let unavailable reason =
  Ok
    {
      status = Unavailable;
      provider = None;
      reason = Some reason;
      report = None;
    }

let select_provider () =
  let platform = Lab.platform_of_host () in
  let probe = Lab.system_probe () in
  match Sys.getenv_opt "DESHELL_OBSERVER_PROVIDER" with
  | Some "none" -> Error "observer disabled by DESHELL_OBSERVER_PROVIDER=none"
  | Some value ->
      begin match provider_of_string value with
      | Error _ as error -> error
      | Ok provider ->
          begin match Lab.validate_provider ~platform probe provider with
          | Ok () -> Ok provider
          | Error _ as error -> error
          end
      end
  | None -> Lab.select ~platform probe

let classify_report provider report =
  if report.Differential.verified then
    {
      status = Verified;
      provider = Some (Lab.provider_to_string provider);
      reason = None;
      report = Some report;
    }
  else
    let failure =
      List.find_map
        (function
          | Differential.Failed { side; message } ->
              let side =
                match side with
                | Differential.Original -> "original"
                | Differential.Migrated -> "migrated"
              in
              Some (side ^ ": " ^ message)
          | Differential.Equivalent _ | Differential.Different _ -> None)
        report.results
    in
    match failure with
    | Some reason ->
        {
          status = Failed;
          provider = Some (Lab.provider_to_string provider);
          reason = Some reason;
          report = Some report;
        }
    | None ->
        {
          status = Different;
          provider = Some (Lab.provider_to_string provider);
          reason = Some "one or more observation dimensions differ";
          report = Some report;
        }

let run ~root ~entry ~plan =
  match Lockfile.load ~root with
  | Error errors -> Error (String.concat "; " errors)
  | Ok lock ->
      begin match Lockfile.observation_image lock with
      | Error reason -> unavailable reason
      | Ok locked_image ->
          let image =
            match Sys.getenv_opt "DESHELL_LAB_IMAGE" with
            | None -> Ok locked_image
            | Some value when value = locked_image -> Ok value
            | Some _ ->
                Error
                  "DESHELL_LAB_IMAGE must exactly match deshell.lock lab.image"
          in
          begin match image with
          | Error _ as error -> error
          | Ok image ->
              begin match select_provider () with
              | Error reason -> unavailable reason
              | Ok provider ->
                  let scenarios_path =
                    Filename.concat
                      (Filename.concat root ".deshell")
                      "scenarios"
                  in
                  begin match Scenario.load_directory scenarios_path with
                  | Error errors -> Error (String.concat "; " errors)
                  | Ok [] ->
                      Error "at least one observation scenario is required"
                  | Ok scenarios ->
                      begin match
                        Observer.verify ~launch:Observer.launch_system ~provider
                          ~root ~entry ~plan ~scenarios ~image
                      with
                      | Error message ->
                          Ok
                            {
                              status = Failed;
                              provider = Some (Lab.provider_to_string provider);
                              reason = Some message;
                              report = None;
                            }
                      | Ok report -> Ok (classify_report provider report)
                      end
                  end
              end
          end
      end

let result_to_yojson outcome =
  let fields =
    [
      ("requested", `Bool true);
      ("status", `String (status_to_string outcome.status));
      ( "provider",
        Option.fold ~none:`Null
          ~some:(fun value -> `String value)
          outcome.provider );
      ( "reason",
        Option.fold ~none:`Null
          ~some:(fun value -> `String value)
          outcome.reason );
    ]
  in
  let fields =
    match outcome.report with
    | None -> fields
    | Some report ->
        ("digest", `String report.digest)
        :: ( "scenarios",
             `List (List.map (fun value -> `String value) report.scenarios) )
        :: fields
  in
  `Assoc fields

let blocks_migration outcome =
  match outcome.status with
  | Different | Failed -> true
  | Verified | Unavailable -> false
