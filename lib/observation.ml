type process = { argv : string list; exit_code : int; parent : int option }

type file_effect = {
  path : string;
  before : string option;
  after : string option;
}

type network_effect = {
  method_ : string;
  uri : string;
  request_digest : string;
  response_digest : string;
  status : int;
}

type t = {
  exit_code : int;
  stdout : string;
  stderr : string;
  timed_out : bool;
  signal : int option;
  processes : process list;
  files : file_effect list;
  network : network_effect list;
}

type difference =
  | Exit_code
  | Stdout
  | Stderr
  | Timeout
  | Signal
  | Process_tree
  | Filesystem
  | Network

type comparison = {
  equivalent : bool;
  differences : difference list;
  expected_digest : string;
  actual_digest : string;
}

let option_to_yojson encode = function
  | None -> `Null
  | Some value -> encode value

let string_list values = `List (List.map (fun value -> `String value) values)

let process_to_yojson process =
  `Assoc
    [
      ("argv", string_list process.argv);
      ("exit_code", `Int process.exit_code);
      ("parent", option_to_yojson (fun value -> `Int value) process.parent);
    ]

let file_to_yojson file =
  `Assoc
    [
      ("path", `String file.path);
      ("before", option_to_yojson (fun value -> `String value) file.before);
      ("after", option_to_yojson (fun value -> `String value) file.after);
    ]

let network_to_yojson network =
  `Assoc
    [
      ("method", `String network.method_);
      ("uri", `String network.uri);
      ("request_digest", `String network.request_digest);
      ("response_digest", `String network.response_digest);
      ("status", `Int network.status);
    ]

let to_yojson observation =
  `Assoc
    [
      ("version", `Int 1);
      ("exit_code", `Int observation.exit_code);
      ("stdout", `String observation.stdout);
      ("stderr", `String observation.stderr);
      ("timed_out", `Bool observation.timed_out);
      ("signal", option_to_yojson (fun value -> `Int value) observation.signal);
      ("processes", `List (List.map process_to_yojson observation.processes));
      ("files", `List (List.map file_to_yojson observation.files));
      ("network", `List (List.map network_to_yojson observation.network));
    ]

let encode_string observation =
  Yojson.Safe.to_string (to_yojson observation) ^ "\n"

let required path name fields =
  match List.assoc_opt name fields with
  | Some value -> Ok value
  | None -> Error [ path ^ " is missing " ^ name ]

let string path = function
  | `String value -> Ok value
  | _ -> Error [ path ^ " must be a string" ]

let int path = function
  | `Int value -> Ok value
  | _ -> Error [ path ^ " must be an integer" ]

let bool path = function
  | `Bool value -> Ok value
  | _ -> Error [ path ^ " must be a boolean" ]

let option decode path = function
  | `Null -> Ok None
  | value -> Result.map Option.some (decode path value)

let list decode path = function
  | `List values ->
      let rec collect index accumulator = function
        | [] -> Ok (List.rev accumulator)
        | value :: rest ->
            begin match decode (Printf.sprintf "%s[%d]" path index) value with
            | Error _ as error -> error
            | Ok decoded -> collect (index + 1) (decoded :: accumulator) rest
            end
      in
      collect 0 [] values
  | _ -> Error [ path ^ " must be an array" ]

let field fields path name decode =
  match required path name fields with
  | Error _ as error -> error
  | Ok value -> decode (path ^ "." ^ name) value

let decode_process path = function
  | `Assoc fields ->
      begin match field fields path "argv" (list string) with
      | Error _ as error -> error
      | Ok argv ->
          begin match field fields path "exit_code" int with
          | Error _ as error -> error
          | Ok exit_code ->
              begin match field fields path "parent" (option int) with
              | Error _ as error -> error
              | Ok parent -> Ok { argv; exit_code; parent }
              end
          end
      end
  | _ -> Error [ path ^ " must be an object" ]

let decode_file path = function
  | `Assoc fields ->
      begin match field fields path "path" string with
      | Error _ as error -> error
      | Ok file_path ->
          begin match field fields path "before" (option string) with
          | Error _ as error -> error
          | Ok before ->
              begin match field fields path "after" (option string) with
              | Error _ as error -> error
              | Ok after -> Ok { path = file_path; before; after }
              end
          end
      end
  | _ -> Error [ path ^ " must be an object" ]

let decode_network path = function
  | `Assoc fields ->
      begin match field fields path "method" string with
      | Error _ as error -> error
      | Ok method_ ->
          begin match field fields path "uri" string with
          | Error _ as error -> error
          | Ok uri ->
              begin match field fields path "request_digest" string with
              | Error _ as error -> error
              | Ok request_digest ->
                  begin match field fields path "response_digest" string with
                  | Error _ as error -> error
                  | Ok response_digest ->
                      begin match field fields path "status" int with
                      | Error _ as error -> error
                      | Ok status ->
                          Ok
                            {
                              method_;
                              uri;
                              request_digest;
                              response_digest;
                              status;
                            }
                      end
                  end
              end
          end
      end
  | _ -> Error [ path ^ " must be an object" ]

let of_yojson = function
  | `Assoc fields ->
      begin match field fields "observation" "version" int with
      | Ok 1 ->
          begin match field fields "observation" "exit_code" int with
          | Error _ as error -> error
          | Ok exit_code ->
              begin match field fields "observation" "stdout" string with
              | Error _ as error -> error
              | Ok stdout ->
                  begin match field fields "observation" "stderr" string with
                  | Error _ as error -> error
                  | Ok stderr ->
                      begin match
                        field fields "observation" "timed_out" bool
                      with
                      | Error _ as error -> error
                      | Ok timed_out ->
                          begin match
                            field fields "observation" "signal" (option int)
                          with
                          | Error _ as error -> error
                          | Ok signal ->
                              begin match
                                field fields "observation" "processes"
                                  (list decode_process)
                              with
                              | Error _ as error -> error
                              | Ok processes ->
                                  begin match
                                    field fields "observation" "files"
                                      (list decode_file)
                                  with
                                  | Error _ as error -> error
                                  | Ok files ->
                                      begin match
                                        field fields "observation" "network"
                                          (list decode_network)
                                      with
                                      | Error _ as error -> error
                                      | Ok network ->
                                          Ok
                                            {
                                              exit_code;
                                              stdout;
                                              stderr;
                                              timed_out;
                                              signal;
                                              processes;
                                              files;
                                              network;
                                            }
                                      end
                                  end
                              end
                          end
                      end
                  end
              end
          end
      | Ok version ->
          Error [ Printf.sprintf "unsupported observation version: %d" version ]
      | Error _ as error -> error
      end
  | _ -> Error [ "observation must be an object" ]

let decode_string source =
  try Yojson.Safe.from_string source |> of_yojson
  with Yojson.Json_error message ->
    Error [ "invalid observation JSON: " ^ message ]

let dimension = function
  | Exit_code -> "exit_code"
  | Stdout -> "stdout"
  | Stderr -> "stderr"
  | Timeout -> "timeout"
  | Signal -> "signal"
  | Process_tree -> "process_tree"
  | Filesystem -> "filesystem"
  | Network -> "network"

let digest observation =
  observation |> to_yojson |> Yojson.Safe.to_string |> Sha256.hex

let compare ~expected ~actual =
  let differences =
    []
    |> (fun values ->
    if expected.exit_code = actual.exit_code then values
    else Exit_code :: values)
    |> (fun values ->
    if expected.stdout = actual.stdout then values else Stdout :: values)
    |> (fun values ->
    if expected.stderr = actual.stderr then values else Stderr :: values)
    |> (fun values ->
    if expected.timed_out = actual.timed_out then values else Timeout :: values)
    |> (fun values ->
    if expected.signal = actual.signal then values else Signal :: values)
    |> (fun values ->
    if expected.processes = actual.processes then values
    else Process_tree :: values)
    |> (fun values ->
    if expected.files = actual.files then values else Filesystem :: values)
    |> (fun values ->
    if expected.network = actual.network then values else Network :: values)
    |> List.rev
  in
  {
    equivalent = differences = [];
    differences;
    expected_digest = digest expected;
    actual_digest = digest actual;
  }

let of_runner (observation : Runner.observation) =
  let processes, files, network =
    List.fold_left
      (fun (processes, files, network) -> function
        | Runner.Process (argv, exit_code) ->
            ({ argv; exit_code; parent = None } :: processes, files, network)
        | Runner.File_write path | Runner.File_remove path ->
            (processes, { path; before = None; after = None } :: files, network)
        | Runner.Network (method_, uri) ->
            ( processes,
              files,
              {
                method_;
                uri;
                request_digest = Sha256.hex "";
                response_digest = Sha256.hex "";
                status = 0;
              }
              :: network )
        | Runner.File_read _ | Runner.Capsule _ -> (processes, files, network))
      ([], [], []) observation.trace
  in
  {
    exit_code = observation.exit_code;
    stdout = observation.stdout;
    stderr = observation.stderr;
    timed_out = false;
    signal = None;
    processes = List.rev processes;
    files = List.rev files;
    network = List.rev network;
  }
