type expectation = Existing of string | Missing

type proposal = {
  path : string;
  expected : expectation;
  replacement : string;
  permissions : int;
}

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let prepare ~path ~replacement =
  let metadata = Unix.lstat path in
  if metadata.Unix.st_kind <> Unix.S_REG then
    invalid_arg ("patch target is not a regular file: " ^ path);
  let contents = read_file path in
  {
    path;
    expected = Existing (Sha256.hex contents);
    replacement;
    permissions = metadata.Unix.st_perm;
  }

let prepare_create ~path ~replacement ~permissions =
  if Sys.file_exists path then
    invalid_arg ("create target already exists: " ^ path);
  { path; expected = Missing; replacement; permissions }

let apply proposal =
  try
    let valid =
      match proposal.expected with
      | Existing expected_hash ->
          let metadata = Unix.lstat proposal.path in
          if metadata.Unix.st_kind <> Unix.S_REG then
            Error ("patch target is no longer a regular file: " ^ proposal.path)
          else
            let current_hash = Sha256.hex (read_file proposal.path) in
            if current_hash <> expected_hash then
              Error
                (Printf.sprintf
                   "content hash mismatch for %s (expected %s, found %s)"
                   proposal.path expected_hash current_hash)
            else Ok ()
      | Missing ->
          begin try
            ignore (Unix.lstat proposal.path);
            Error ("create target now exists: " ^ proposal.path)
          with Unix.Unix_error (Unix.ENOENT, _, _) -> Ok ()
          end
    in
    match valid with
    | Error _ as error -> error
    | Ok () ->
        let directory = Filename.dirname proposal.path in
        let temporary =
          Filename.temp_file ~temp_dir:directory ".deshell-patch-" ".tmp"
        in
        let channel = open_out_bin temporary in
        begin try
          output_string channel proposal.replacement;
          close_out channel;
          Unix.chmod temporary proposal.permissions;
          Unix.rename temporary proposal.path;
          Ok ()
        with error ->
          close_out_noerr channel;
          (try Sys.remove temporary with _ -> ());
          raise error
        end
  with
  | Sys_error message -> Error message
  | Unix.Unix_error (error, function_name, argument) ->
      Error
        (Printf.sprintf "%s(%s): %s" function_name argument
           (Unix.error_message error))

type validated = {
  proposal : proposal;
  original : string option;
  canonical : string;
}

let error_of_exception = function
  | Sys_error message -> message
  | Unix.Unix_error (error, function_name, argument) ->
      Printf.sprintf "%s(%s): %s" function_name argument
        (Unix.error_message error)
  | error -> Printexc.to_string error

let validate_proposal proposal =
  try
    match proposal.expected with
    | Existing expected_hash ->
        let canonical = Unix.realpath proposal.path in
        let metadata = Unix.lstat canonical in
        if metadata.Unix.st_kind <> Unix.S_REG then
          Error ("patch target is not a regular file: " ^ proposal.path)
        else
          let original = read_file canonical in
          let actual_hash = Sha256.hex original in
          if actual_hash <> expected_hash then
            Error
              (Printf.sprintf
                 "content hash mismatch for %s (expected %s, found %s)"
                 proposal.path expected_hash actual_hash)
          else Ok { proposal; original = Some original; canonical }
    | Missing ->
        begin try
          ignore (Unix.lstat proposal.path);
          Error ("create target already exists: " ^ proposal.path)
        with Unix.Unix_error (Unix.ENOENT, _, _) ->
          let parent = Unix.realpath (Filename.dirname proposal.path) in
          let metadata = Unix.lstat parent in
          if metadata.Unix.st_kind <> Unix.S_DIR then
            Error ("create target parent is not a directory: " ^ parent)
          else
            let canonical =
              Filename.concat parent (Filename.basename proposal.path)
            in
            Ok { proposal; original = None; canonical }
        end
  with error -> Error (error_of_exception error)

let write_temporary ~path ~permissions contents =
  let directory = Filename.dirname path in
  let temporary =
    Filename.temp_file ~temp_dir:directory ".deshell-transaction-" ".tmp"
  in
  try
    let channel = open_out_bin temporary in
    begin try
      output_string channel contents;
      close_out channel;
      Unix.chmod temporary permissions;
      Ok temporary
    with error ->
      close_out_noerr channel;
      raise error
    end
  with error ->
    (try Sys.remove temporary with _ -> ());
    Error (error_of_exception error)

let apply_all proposals =
  let rec validate seen accumulator = function
    | [] -> Ok (List.rev accumulator)
    | proposal :: rest ->
        begin match validate_proposal proposal with
        | Error _ as error -> error
        | Ok validated ->
            if List.mem validated.canonical seen then
              Error ("duplicate patch target: " ^ validated.canonical)
            else
              validate
                (validated.canonical :: seen)
                (validated :: accumulator) rest
        end
  in
  let cleanup paths =
    List.iter
      (fun path ->
        if Sys.file_exists path then try Sys.remove path with _ -> ())
      paths
  in
  match validate [] [] proposals with
  | Error _ as error -> error
  | Ok validated ->
      let rec stage accumulator = function
        | [] -> Ok (List.rev accumulator)
        | item :: rest ->
            begin match
              write_temporary ~path:item.canonical
                ~permissions:item.proposal.permissions item.proposal.replacement
            with
            | Error message ->
                cleanup (List.map snd accumulator);
                Error message
            | Ok temporary -> stage ((item, temporary) :: accumulator) rest
            end
      in
      begin match stage [] validated with
      | Error _ as error -> error
      | Ok staged ->
          (* Validate the complete read set again after staging. No target is
             mutated unless every content hash still matches. *)
          let stale =
            List.find_opt
              (fun (item, _) ->
                match item.proposal.expected with
                | Existing expected_hash ->
                    begin try
                      Sha256.hex (read_file item.canonical) <> expected_hash
                    with _ -> true
                    end
                | Missing ->
                    begin try
                      ignore (Unix.lstat item.canonical);
                      true
                    with Unix.Unix_error (Unix.ENOENT, _, _) -> false
                    end)
              staged
          in
          begin match stale with
          | Some (item, _) ->
              cleanup (List.map snd staged);
              Error ("content hash changed while staging: " ^ item.canonical)
          | None ->
              let rec restore restored_errors = function
                | [] -> List.rev restored_errors
                | item :: rest ->
                    begin match item.original with
                    | None ->
                        begin try
                          if Sys.file_exists item.canonical then
                            Sys.remove item.canonical;
                          restore restored_errors rest
                        with error ->
                          restore
                            (error_of_exception error :: restored_errors)
                            rest
                        end
                    | Some original ->
                        begin match
                          write_temporary ~path:item.canonical
                            ~permissions:item.proposal.permissions original
                        with
                        | Error message ->
                            restore (message :: restored_errors) rest
                        | Ok temporary ->
                            begin try
                              Unix.rename temporary item.canonical;
                              restore restored_errors rest
                            with error ->
                              cleanup [ temporary ];
                              restore
                                (error_of_exception error :: restored_errors)
                                rest
                            end
                        end
                    end
              in
              let rec commit committed = function
                | [] -> Ok ()
                | (item, temporary) :: rest ->
                    begin try
                      Unix.rename temporary item.canonical;
                      commit (item :: committed) rest
                    with error ->
                      cleanup (List.map snd rest @ [ temporary ]);
                      let restore_errors = restore [] committed in
                      let message = error_of_exception error in
                      if restore_errors = [] then Error message
                      else
                        Error
                          (message ^ "; rollback failed: "
                          ^ String.concat "; " restore_errors)
                    end
              in
              commit [] staged
          end
      end
