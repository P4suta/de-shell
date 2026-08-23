let fail message =
  prerr_endline message;
  2

let main () =
  match Deshell.Process_backend.read_channel_limited stdin with
  | Error message -> fail message
  | Ok source ->
      begin match Deshell.Process_backend.decode_agent_invocation source with
      | Error message -> fail message
      | Ok invocation ->
          begin try
            let cwd = Unix.realpath invocation.cwd in
            if not (Sys.is_directory cwd) then
              fail ("process-agent cwd is not a directory: " ^ cwd)
            else begin
              Unix.chdir cwd;
              match
                Deshell.Process_backend.execute_local invocation.request
              with
              | Error message -> fail message
              | Ok result ->
                  print_string
                    (Deshell.Process_backend.encode_agent_result result);
                  flush stdout;
                  0
            end
          with
          | Sys_error message -> fail message
          | Unix.Unix_error (error, function_name, argument) ->
              fail
                (Printf.sprintf "%s(%s): %s" function_name argument
                   (Unix.error_message error))
          end
      end

let () = exit (main ())
