type candidate = { provider : string; rationale : string; plan : Ir.plan }
type accepted = { candidate : candidate; report : Differential.report }

let decode_candidate = function
  | `Assoc fields ->
      let errors = ref [] in
      let string name =
        match List.assoc_opt name fields with
        | Some (`String value) when value <> "" -> value
        | _ ->
            errors :=
              ("synthesizer candidate " ^ name ^ " is required") :: !errors;
            ""
      in
      let provider = string "provider" in
      let rationale = string "rationale" in
      let plan =
        match List.assoc_opt "plan" fields with
        | None ->
            errors := "synthesizer candidate plan is required" :: !errors;
            None
        | Some value ->
            begin match Ir_codec.decode_yojson value with
            | Ok plan ->
                begin match Ir.validate_plan plan with
                | Ok () -> Some plan
                | Error validation_errors ->
                    errors := List.rev_append validation_errors !errors;
                    None
                end
            | Error decode_errors ->
                errors := List.rev_append decode_errors !errors;
                None
            end
      in
      begin match (List.rev !errors, plan) with
      | [], Some plan -> Ok { provider; rationale; plan }
      | errors, _ -> Error errors
      end
  | _ -> Error [ "synthesizer candidate must be a JSON object" ]

let request client ~path ~source =
  match
    Adapter_client.call client ~method_:"synthesizer.propose"
      ~params:
        (`Assoc
           [
             ("path", `String path);
             ("source", `String source);
             ("effect_ir_schema", `Int Ir.current_schema_version);
           ])
  with
  | Error message -> Error [ message ]
  | Ok response -> decode_candidate response

let valid_digest value =
  String.length value = 64
  && String.for_all
       (function '0' .. '9' | 'a' .. 'f' -> true | _ -> false)
       value

let validate ~verify candidate =
  match Ir.validate_plan candidate.plan with
  | Error errors -> Error (String.concat "; " errors)
  | Ok () ->
      begin match verify candidate.plan with
      | Error _ as error -> error
      | Ok report when not report.Differential.verified ->
          Error
            "synthesized candidate failed the ordinary differential \
             verification gate"
      | Ok report when report.scenarios = [] ->
          Error "synthesized candidate verification has no scenarios"
      | Ok report when not (valid_digest report.digest) ->
          Error "synthesized candidate verification digest is invalid"
      | Ok report -> Ok { candidate; report }
      end
