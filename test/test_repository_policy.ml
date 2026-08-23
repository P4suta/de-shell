open Alcotest

let root =
  match Sys.getenv_opt "DESHELL_REPOSITORY_ROOT" with
  | Some value -> value
  | None -> failwith "DESHELL_REPOSITORY_ROOT is not set"

let path relative = Filename.concat root relative

let read relative =
  let channel = open_in_bin (path relative) in
  Fun.protect
    ~finally:(fun () -> close_in_noerr channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let substring_index haystack needle =
  let haystack_length = String.length haystack in
  let needle_length = String.length needle in
  let rec loop index =
    if index + needle_length > haystack_length then None
    else if String.sub haystack index needle_length = needle then Some index
    else loop (index + 1)
  in
  if needle_length = 0 then Some 0 else loop 0

let contains haystack needle = Option.is_some (substring_index haystack needle)

let required_files () =
  [
    ".editorconfig";
    ".gitattributes";
    "CITATION.cff";
    ".github/CODEOWNERS";
    ".github/dependabot.yml";
    ".github/pull_request_template.md";
    ".github/ISSUE_TEMPLATE/bug_report.yml";
    ".github/ISSUE_TEMPLATE/feature_request.yml";
    ".github/ISSUE_TEMPLATE/config.yml";
    ".github/release.yml";
    ".github/workflows/ci.yml";
    ".github/workflows/dependency-review.yml";
    ".github/workflows/scorecard.yml";
    ".github/rulesets/default-branch.json";
    ".github/rulesets/release-tags.json";
    ".github/settings/repository.json";
    ".github/settings/capabilities.json";
    ".github/settings/topics.json";
    ".github/settings/actions.json";
    ".github/settings/selected-actions.json";
    ".github/settings/workflow-permissions.json";
    ".github/settings/fork-approval.json";
    ".github/settings/features.json";
    ".github/settings/labels.json";
    "adapters/nushell/dune";
    "scripts/github-repository.ps1";
    "scripts/repository-guardrails.ps1";
    "SECURITY.md";
    "SUPPORT.md";
    "CODE_OF_CONDUCT.md";
    "GOVERNANCE.md";
    "CHANGELOG.md";
  ]
  |> List.iter (fun relative ->
      check bool relative true (Sys.file_exists (path relative)))

let is_hex character =
  match character with
  | '0' .. '9' | 'a' .. 'f' | 'A' .. 'F' -> true
  | _ -> false

let action_reference line =
  match String.index_opt line '@' with
  | None -> None
  | Some start ->
      let rec finish index =
        if index >= String.length line then index
        else
          match line.[index] with
          | ' ' | '\t' | '#' | '\r' -> index
          | _ -> finish (index + 1)
      in
      let first = start + 1 in
      let last = finish first in
      Some (String.sub line first (last - first))

let workflow_actions_are_pinned () =
  [
    ".github/workflows/ci.yml";
    ".github/workflows/dependency-review.yml";
    ".github/workflows/scorecard.yml";
  ]
  |> List.iter (fun relative ->
      let contents = read relative in
      check bool
        (relative ^ " declares permissions")
        true
        (contains contents "permissions:");
      check bool
        (relative ^ " avoids pull_request_target")
        false
        (contains contents "pull_request_target");
      String.split_on_char '\n' contents
      |> List.filter (fun line -> contains line "uses:")
      |> List.iter (fun line ->
          match action_reference line with
          | None -> fail (relative ^ " has an action without a ref")
          | Some reference ->
              check int
                (relative ^ " action SHA length")
                40 (String.length reference);
              check bool
                (relative ^ " action SHA is hexadecimal")
                true
                (String.for_all is_hex reference)))

let json relative = Yojson.Safe.from_file (path relative)
let member name value = Yojson.Safe.Util.member name value

let strings value =
  Yojson.Safe.Util.to_list value |> List.map Yojson.Safe.Util.to_string

let rule_types ruleset =
  member "rules" ruleset |> Yojson.Safe.Util.to_list
  |> List.map (fun rule -> member "type" rule |> Yojson.Safe.Util.to_string)

let default_branch_ruleset () =
  let ruleset = json ".github/rulesets/default-branch.json" in
  check string "branch target" "branch"
    (member "target" ruleset |> Yojson.Safe.Util.to_string);
  check string "active enforcement" "active"
    (member "enforcement" ruleset |> Yojson.Safe.Util.to_string);
  let includes =
    member "conditions" ruleset
    |> member "ref_name" |> member "include" |> strings
  in
  check bool "default branch selected" true
    (List.mem "~DEFAULT_BRANCH" includes);
  let types = rule_types ruleset in
  [
    "deletion";
    "non_fast_forward";
    "required_linear_history";
    "required_signatures";
    "pull_request";
    "required_status_checks";
  ]
  |> List.iter (fun rule ->
      check bool ("branch rule " ^ rule) true (List.mem rule types));
  let status_rule =
    member "rules" ruleset |> Yojson.Safe.Util.to_list
    |> List.find (fun rule ->
        member "type" rule = `String "required_status_checks")
  in
  let contexts =
    member "parameters" status_rule
    |> member "required_status_checks"
    |> Yojson.Safe.Util.to_list
    |> List.map (fun item ->
        member "context" item |> Yojson.Safe.Util.to_string)
  in
  check bool "stable required CI gate" true (List.mem "Required gate" contexts)

let release_tag_ruleset () =
  let ruleset = json ".github/rulesets/release-tags.json" in
  check string "tag target" "tag"
    (member "target" ruleset |> Yojson.Safe.Util.to_string);
  check string "tag enforcement" "active"
    (member "enforcement" ruleset |> Yojson.Safe.Util.to_string);
  let includes =
    member "conditions" ruleset
    |> member "ref_name" |> member "include" |> strings
  in
  check (list string) "release tag namespace" [ "refs/tags/v*" ] includes;
  let types = rule_types ruleset in
  [ "deletion"; "non_fast_forward"; "required_signatures" ]
  |> List.iter (fun rule ->
      check bool ("release tag rule " ^ rule) true (List.mem rule types))

let push_guardrail_fallback () =
  let capabilities = json ".github/settings/capabilities.json" in
  let push_ruleset = member "push_ruleset" capabilities in
  check string "push Ruleset capability" "unavailable"
    (member "status" push_ruleset |> Yojson.Safe.Util.to_string);
  check int "fallback maximum file size" 10
    (member "max_file_size_mib" push_ruleset |> Yojson.Safe.Util.to_int);
  check int "fallback maximum path length" 240
    (member "max_file_path_length" push_ruleset |> Yojson.Safe.Util.to_int);
  let mise = read "mise.toml" in
  check bool "mise exposes repository guardrail task" true
    (contains mise "[tasks.\"repository:guardrails\"]");
  check bool "lint executes repository guardrails" true
    (contains mise "mise run repository:guardrails")

let repository_security_settings () =
  let actions = json ".github/settings/actions.json" in
  check bool "Actions enabled" true
    (member "enabled" actions |> Yojson.Safe.Util.to_bool);
  check string "selected Actions only" "selected"
    (member "allowed_actions" actions |> Yojson.Safe.Util.to_string);
  check bool "Action SHA pinning enforced" true
    (member "sha_pinning_required" actions |> Yojson.Safe.Util.to_bool);
  let repository = json ".github/settings/repository.json" in
  let security = member "security_and_analysis" repository in
  [ "secret_scanning"; "secret_scanning_push_protection" ]
  |> List.iter (fun setting ->
      check string setting "enabled"
        (member setting security |> member "status"
       |> Yojson.Safe.Util.to_string))

let github_reconciliation_handles_unordered_topics () =
  let script = read "scripts/github-repository.ps1" in
  check bool "topics are compared as an unordered string set" true
    (contains script
       "Assert-StringSetEqual -Expected $expectedTopics.names -Actual \
        $topicsState.names")

let ci_bootstraps_platform_dependencies () =
  let ci = read ".github/workflows/ci.yml" in
  check bool "mise Rust is materialized before opam" true
    (contains ci "rustc --version"
    && contains ci "cargo --version"
    && Option.value ~default:max_int (substring_index ci "cargo --version")
       < Option.value ~default:max_int
           (substring_index ci "run: mise run setup"));
  check bool "Linux installs the opam sandbox dependency" true
    (contains ci "sudo apt-get install --yes apparmor bubblewrap");
  check bool "Linux-only package installation" true
    (contains ci "if: runner.os == 'Linux'");
  check bool "Linux keeps opam bubblewrap sandboxing enabled" true
    (contains ci "apparmor_parser --replace" && contains ci "userns,");
  let mise = read "mise.toml" in
  check bool "mise-owned depexts are explicit to opam" true
    (contains mise "--assume-depexts");
  check bool "Unix sandbox failure is fail-closed" true
    (contains mise "opam init --cli=2.5 --bare --no-setup --no");
  check bool "Windows preserves opam-managed MinGW depexts" true
    (contains mise "run_windows = ["
    && contains mise
         "--with-dev-setup --locked --yes || opam install . --cli=2.5 \
          --switch=.")

let rust_adapter_package_rule_is_hermetic () =
  let dune = read "adapters/nushell/dune" in
  check bool "Cargo output is a declared directory target" true
    (contains dune "(dir cargo-target)");
  check bool "Cargo uses an explicit target directory" true
    (contains dune "--target-dir");
  check bool "generated Cargo artifact is not a static copy dependency" false
    (contains dune "(copy target/release")

let ownership_and_ci () =
  let owners = read ".github/CODEOWNERS" in
  check bool "global code owner" true (contains owners "* @P4suta");
  check bool "workflow code owner" true
    (contains owners "/.github/workflows/ @P4suta");
  let ci = read ".github/workflows/ci.yml" in
  check bool "stable required gate job" true (contains ci "name: Required gate");
  check bool "CI concurrency" true (contains ci "concurrency:");
  check bool "checkout credentials disabled" true
    (contains ci "persist-credentials: false")

let suite =
  [
    test_case "required repository files" `Quick required_files;
    test_case "workflow actions use immutable SHAs" `Quick
      workflow_actions_are_pinned;
    test_case "default branch ruleset" `Quick default_branch_ruleset;
    test_case "release tag ruleset" `Quick release_tag_ruleset;
    test_case "push guardrail fallback" `Quick push_guardrail_fallback;
    test_case "repository security settings" `Quick repository_security_settings;
    test_case "unordered GitHub topics" `Quick
      github_reconciliation_handles_unordered_topics;
    test_case "platform dependency bootstrap" `Quick
      ci_bootstraps_platform_dependencies;
    test_case "hermetic Rust adapter package" `Quick
      rust_adapter_package_rule_is_hermetic;
    test_case "ownership and stable CI" `Quick ownership_and_ci;
  ]

let () = Alcotest.run "repository policy" [ ("github", suite) ]
