let required_paths root =
  let executable name = name ^ if Sys.win32 then ".exe" else "" in
  [
    Filename.concat "bin" (executable "deshell");
    Filename.concat "bin" (executable "deshell-observer-agent");
    Filename.concat "bin" (executable "deshell-process-agent");
    Filename.concat "lib/de-shell" "deshell-powershell-adapter.ps1";
    Filename.concat "lib/de-shell" "deshell-audit-corpus.ps1";
    Filename.concat "lib/de-shell" "deshell-nushell-adapter.exe";
    Filename.concat "share/de-shell" "effect-ir.schema.json";
    Filename.concat "share/de-shell" "evidence.schema.json";
    Filename.concat "share/de-shell" "adapter.schema.json";
    Filename.concat "share/de-shell" "corpus-audit.schema.json";
  ]
  |> List.map (Filename.concat root)

let () =
  let root =
    if Array.length Sys.argv = 2 then Sys.argv.(1)
    else begin
      prerr_endline "usage: package_check INSTALL_ROOT";
      exit 2
    end
  in
  let missing =
    required_paths root
    |> List.filter (fun path ->
        (not (Sys.file_exists path)) || Sys.is_directory path)
  in
  match missing with
  | [] -> print_endline "package payload is complete"
  | paths ->
      List.iter
        (fun path -> prerr_endline ("missing package file: " ^ path))
        paths;
      exit 1
