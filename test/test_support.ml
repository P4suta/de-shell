let write_file path contents =
  let channel = open_out_bin path in
  Fun.protect
    ~finally:(fun () -> close_out_noerr channel)
    (fun () -> output_string channel contents)

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let rec remove_tree path =
  match Unix.lstat path with
  | exception Unix.Unix_error (Unix.ENOENT, _, _) -> ()
  | { Unix.st_kind = Unix.S_DIR; _ } ->
      Sys.readdir path
      |> Array.iter (fun name -> remove_tree (Filename.concat path name));
      Unix.rmdir path
  | _ -> Sys.remove path

let with_temp_dir f =
  let marker = Filename.temp_file "deshell-test-" "" in
  Sys.remove marker;
  Unix.mkdir marker 0o700;
  Fun.protect ~finally:(fun () -> remove_tree marker) (fun () -> f marker)

type process_result = { status : int; stdout : string; stderr : string }

let run_process executable arguments =
  let stdout_path = Filename.temp_file "deshell-stdout-" ".txt" in
  let stderr_path = Filename.temp_file "deshell-stderr-" ".txt" in
  let stdout_fd =
    Unix.openfile stdout_path [ Unix.O_WRONLY; Unix.O_TRUNC ] 0o600
  in
  let stderr_fd =
    Unix.openfile stderr_path [ Unix.O_WRONLY; Unix.O_TRUNC ] 0o600
  in
  let argv = Array.of_list (executable :: arguments) in
  let pid =
    Unix.create_process executable argv Unix.stdin stdout_fd stderr_fd
  in
  Unix.close stdout_fd;
  Unix.close stderr_fd;
  let _, process_status = Unix.waitpid [] pid in
  let stdout = read_file stdout_path in
  let stderr = read_file stderr_path in
  Sys.remove stdout_path;
  Sys.remove stderr_path;
  let status =
    match process_status with
    | Unix.WEXITED code -> code
    | Unix.WSIGNALED signal | Unix.WSTOPPED signal -> 128 + signal
  in
  { status; stdout; stderr }

let contains ~needle haystack =
  let needle_length = String.length needle in
  let haystack_length = String.length haystack in
  let rec loop index =
    if index + needle_length > haystack_length then false
    else if String.sub haystack index needle_length = needle then true
    else loop (index + 1)
  in
  needle_length = 0 || loop 0
