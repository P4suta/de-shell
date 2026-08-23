let interpreter_for_extension path =
  match Filename.extension path |> String.lowercase_ascii with
  | ".sh" -> Some "sh"
  | ".bash" -> Some "bash"
  | ".zsh" -> Some "zsh"
  | ".fish" -> Some "fish"
  | ".ps1" | ".psm1" -> Some "powershell"
  | ".cmd" | ".bat" -> Some "cmd"
  | ".nu" -> Some "nu"
  | _ -> None

let detect ~path ~source =
  match interpreter_for_extension path with
  | Some interpreter -> interpreter
  | None ->
      Option.value ~default:"unknown" (Scanner.interpreter_for_shebang source)

let lower ~path source =
  let interpreter = detect ~path ~source in
  match interpreter with
  | "sh" | "bash" | "dash" | "ksh" -> Posix_frontend.lower ~path source
  | "zsh" -> Posix_frontend.lower ~path source
  | "fish" -> Literal_frontend.lower Literal_frontend.Fish ~path source
  | "powershell" | "pwsh" ->
      Literal_frontend.lower Literal_frontend.Powershell ~path source
  | "cmd" -> Literal_frontend.lower Literal_frontend.Cmd ~path source
  | "nu" | "nushell" -> Literal_frontend.lower Literal_frontend.Nu ~path source
  | _ ->
      Posix_frontend.residual ~interpreter ~path ~source
        ~reason:
          (Printf.sprintf
             "%s frontend is trace-only in this build; unobserved behavior is \
              not claimed as verified"
             interpreter)
        ()
