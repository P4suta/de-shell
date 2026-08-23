let usage () =
  prerr_endline
    "usage: deshell-observer-agent (--request-base64 DATA | --workspace DIR \
     --result FILE --timeout-ms MS --interpreter NAME --script PATH [-- \
     ARGS...])";
  2

let parse () =
  if Array.length Sys.argv = 3 && Sys.argv.(1) = "--request-base64" then
    Deshell.Observer_agent.decode_invocation Sys.argv.(2)
  else
    let workspace = ref None in
    let result = ref None in
    let timeout_ms = ref None in
    let interpreter = ref None in
    let script = ref None in
    let rec loop index =
      if index >= Array.length Sys.argv then Ok []
      else
        match Sys.argv.(index) with
        | "--" ->
            Ok
              (List.init
                 (Array.length Sys.argv - index - 1)
                 (fun offset -> Sys.argv.(index + offset + 1)))
        | ( "--workspace" | "--result" | "--timeout-ms" | "--interpreter"
          | "--script" ) as option ->
            if index + 1 >= Array.length Sys.argv then
              Error ("missing value for " ^ option)
            else begin
              let value = Sys.argv.(index + 1) in
              begin match option with
              | "--workspace" -> workspace := Some value
              | "--result" -> result := Some value
              | "--timeout-ms" ->
                  begin try timeout_ms := Some (int_of_string value)
                  with Failure _ -> ()
                  end
              | "--interpreter" -> interpreter := Some value
              | "--script" -> script := Some value
              | _ -> assert false
              end;
              loop (index + 2)
            end
        | option -> Error ("unknown option: " ^ option)
    in
    match loop 1 with
    | Error _ as error -> error
    | Ok args ->
        begin match
          (!workspace, !result, !timeout_ms, !interpreter, !script)
        with
        | ( Some workspace,
            Some result,
            Some timeout_ms,
            Some interpreter,
            Some script ) ->
            Ok
              Deshell.Observer_agent.
                {
                  workspace;
                  result_path = result;
                  timeout_ms;
                  interpreter;
                  script;
                  args;
                  environment = [];
                }
        | _ -> Error "missing required observer-agent option"
        end

let write_file path contents =
  let channel = open_out_bin path in
  Fun.protect
    ~finally:(fun () -> close_out_noerr channel)
    (fun () -> output_string channel contents)

let main () =
  match parse () with
  | Error message ->
      prerr_endline message;
      usage ()
  | Ok invocation ->
      begin try
        Unix.chdir invocation.workspace;
        let root = Unix.realpath "." in
        let argv =
          Deshell.Observer_agent.argv_for_script
            ~interpreter:invocation.interpreter ~script:invocation.script
            invocation.args
        in
        begin match
          Deshell.Observer_agent.run
            ~execute:Deshell.Observer_agent.execute_system ~root ~argv
            ~environment:invocation.environment
            ~timeout_ms:invocation.timeout_ms
        with
        | Error message ->
            prerr_endline message;
            1
        | Ok observation ->
            write_file invocation.result_path
              (Deshell.Observation.encode_string observation);
            0
        end
      with
      | Sys_error message ->
          prerr_endline message;
          1
      | Unix.Unix_error (error, function_name, argument) ->
          prerr_endline
            (Printf.sprintf "%s(%s): %s" function_name argument
               (Unix.error_message error));
          1
      end

let () = exit (main ())
