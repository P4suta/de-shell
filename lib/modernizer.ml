type profile = Portable | Secure | Reproducible
type severity = Info | Warning | High

type finding = {
  rule : string;
  profile : profile;
  severity : severity;
  message : string;
  span : Ir.source_span;
  auto_applicable : bool;
}

type result = {
  output : string;
  edits : Rewrite.edit list;
  findings : finding list;
}

let contains ~needle haystack =
  let needle_length = String.length needle in
  let rec loop index =
    index + needle_length <= String.length haystack
    && (String.sub haystack index needle_length = needle || loop (index + 1))
  in
  needle_length = 0 || loop 0

let position_at source offset =
  let line = ref 1 in
  let column = ref 0 in
  for index = 0 to offset - 1 do
    if source.[index] = '\n' then begin
      incr line;
      column := 0
    end
    else incr column
  done;
  (!line, !column)

let span ~path source start_byte end_byte =
  let start_line, start_column = position_at source start_byte in
  let end_line, end_column = position_at source end_byte in
  Ir.
    {
      file = path;
      start_line;
      start_column;
      end_line;
      end_column;
      start_byte;
      end_byte;
    }

let has_strict_mode source =
  String.split_on_char '\n' source
  |> List.exists (fun line ->
      let line = String.trim line in
      String.starts_with ~prefix:"set -e" line
      || String.starts_with ~prefix:"set -o errexit" line)

let insertion_after_shebang source =
  if String.starts_with ~prefix:"#!" source then
    match String.index_opt source '\n' with
    | Some newline -> newline + 1
    | None -> String.length source
  else 0

let finding ~rule ~profile ~severity ~message ~span ~auto_applicable =
  { rule; profile; severity; message; span; auto_applicable }

let propose ~path ~profiles source =
  let findings = ref [] in
  let edits = ref [] in
  let output = ref source in
  let selected profile = List.mem profile profiles in
  if selected Secure && not (has_strict_mode source) then begin
    let interpreter = Frontend_registry.detect ~path ~source in
    if List.mem interpreter [ "sh"; "bash"; "dash"; "ksh"; "zsh" ] then begin
      let offset = insertion_after_shebang source in
      let replacement = "set -eu\n" in
      let source_span = span ~path source offset offset in
      output :=
        String.sub source 0 offset ^ replacement
        ^ String.sub source offset (String.length source - offset);
      edits :=
        Rewrite.
          { rule = "secure.strict-mode"; original = source_span; replacement }
        :: !edits;
      findings :=
        finding ~rule:"secure.strict-mode" ~profile:Secure ~severity:Warning
          ~message:
            "Enable errexit and nounset; this intentionally changes failure \
             behavior and requires --apply."
          ~span:source_span ~auto_applicable:true
        :: !findings
    end
  end;
  if
    selected Secure
    && contains ~needle:"curl " source
    && (contains ~needle:"| sh" source || contains ~needle:"| bash" source)
  then
    findings :=
      finding ~rule:"secure.remote-code-pipe" ~profile:Secure ~severity:High
        ~message:
          "Download-then-execute pipeline needs a pinned digest and a reviewed \
           two-step replacement."
        ~span:(span ~path source 0 (String.length source))
        ~auto_applicable:false
      :: !findings;
  if selected Secure && contains ~needle:"chmod 777" source then
    findings :=
      finding ~rule:"secure.world-writable" ~profile:Secure ~severity:High
        ~message:
          "World-writable permissions should be replaced with the least \
           required mode."
        ~span:(span ~path source 0 (String.length source))
        ~auto_applicable:false
      :: !findings;
  if selected Portable && contains ~needle:"[[" source then
    findings :=
      finding ~rule:"portable.double-bracket" ~profile:Portable
        ~severity:Warning
        ~message:"[[ ... ]] is not POSIX; review a test/case replacement."
        ~span:(span ~path source 0 (String.length source))
        ~auto_applicable:false
      :: !findings;
  if selected Reproducible && contains ~needle:":latest" source then
    findings :=
      finding ~rule:"reproducible.latest-tag" ~profile:Reproducible
        ~severity:Warning
        ~message:"Replace floating :latest references with an immutable digest."
        ~span:(span ~path source 0 (String.length source))
        ~auto_applicable:false
      :: !findings;
  { output = !output; edits = List.rev !edits; findings = List.rev !findings }
