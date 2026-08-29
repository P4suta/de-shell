use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ZERO_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HardenPlan {
    pub schema_version: u32,
    pub kind: HardenKind,
    pub plan_digest: String,
    pub profile: HardenProfile,
    pub changes: Vec<HardenChange>,
    pub validation_commands: Vec<HardenCommand>,
    pub blockers: Vec<HardenBlocker>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HardenKind {
    Harden,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HardenProfile {
    Secure,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HardenChange {
    pub path: String,
    pub before_digest: String,
    pub after_digest: String,
    pub replacement_base64: String,
    pub permissions: u32,
    pub rules: Vec<HardenRule>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HardenRule {
    pub rule: String,
    pub start_byte: u64,
    pub end_byte: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HardenCommand {
    pub name: String,
    pub kind: crate::config::ValidationKind,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HardenBlocker {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
    pub start_byte: Option<u64>,
    pub end_byte: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HardenApproval {
    pub schema_version: u32,
    pub plan_digest: String,
    pub approval: HardenApprovalState,
    pub owner: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HardenApprovalState {
    Draft,
    Approved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HardenEvidence {
    pub schema_version: u32,
    pub kind: HardenKind,
    pub evidence_digest: String,
    pub plan_digest: String,
    pub approval_digest: String,
    pub platform_fingerprint: String,
    pub status: HardenEvidenceStatus,
    pub changes: Vec<HardenEvidenceChange>,
    pub validation: Vec<HardenValidation>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HardenEvidenceStatus {
    Verified,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HardenEvidenceChange {
    pub path: String,
    pub before_digest: String,
    pub after_digest: String,
    pub rules: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HardenValidation {
    pub name: String,
    pub kind: crate::config::ValidationKind,
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub limit_exceeded: Option<String>,
    pub stdout_digest: String,
    pub stderr_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct HardenApplied {
    schema_version: u32,
    kind: HardenKind,
    plan_digest: String,
    approval_digest: String,
    evidence_digest: String,
    changes: Vec<HardenEvidenceChange>,
}

#[derive(Clone, Debug)]
pub(crate) struct PlanOutput {
    pub digest: String,
    pub diff: String,
    pub blockers: Vec<HardenBlocker>,
    pub approval_path: PathBuf,
}

pub(crate) fn plan(root: &Path) -> Result<PlanOutput, String> {
    let config = crate::project::load_config(root).map_err(|errors| errors.join("; "))?;
    let lock = crate::project::load_lock(root).map_err(|errors| errors.join("; "))?;
    let inventory = crate::project::scan(root)?;
    if !inventory.errors.is_empty() || !inventory.skipped.is_empty() {
        return Err(format!(
            "DESHELL_HARDEN_SCAN_INCOMPLETE: {} error(s), {} skipped path(s)",
            inventory.errors.len(),
            inventory.skipped.len()
        ));
    }
    let mut findings = inventory.findings;
    findings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.span.start_byte.cmp(&right.span.start_byte))
    });
    let mut changes = Vec::new();
    let mut blockers = Vec::new();
    let mut diffs = Vec::new();
    for finding in findings {
        if finding.kind != crate::scanner::FindingKind::ShellFile {
            blockers.push(HardenBlocker {
                code: "DESHELL_HARDEN_STRUCTURED_REVIEW_REQUIRED".into(),
                message: "embedded or dynamic shell hardening requires a structured host proposal"
                    .into(),
                path: Some(finding.path),
                start_byte: Some(finding.span.start_byte),
                end_byte: Some(finding.span.end_byte),
            });
            continue;
        }
        let source = String::from_utf8(finding.source.clone()).map_err(|_| {
            format!(
                "DESHELL_HARDEN_UNSUPPORTED_ENCODING: {} is not UTF-8",
                finding.path
            )
        })?;
        let result =
            crate::rewrite::modernize(&finding.path, &source, &[crate::rewrite::Profile::Secure]);
        for risk in result
            .findings
            .iter()
            .filter(|finding| !finding.auto_applicable)
        {
            blockers.push(HardenBlocker {
                code: "DESHELL_HARDEN_MANUAL_PROPOSAL_REQUIRED".into(),
                message: format!("{}: {}", risk.rule, risk.message),
                path: Some(finding.path.clone()),
                start_byte: Some(risk.span.start_byte),
                end_byte: Some(risk.span.end_byte),
            });
        }
        if result.output == source {
            continue;
        }
        let interpreter = finding
            .interpreter
            .as_deref()
            .ok_or_else(|| format!("DESHELL_HARDEN_INTERPRETER_REQUIRED: {}", finding.path))?;
        let mut lowered = crate::frontend::lower_with_interpreter(
            &finding.path,
            result.output.as_bytes(),
            config.policy.unknown_interpreter.clone(),
            interpreter,
        )?;
        crate::frontend::bind_interpreter_pins(&mut lowered, &lock.interpreters)?;
        let rules = result
            .edits
            .iter()
            .map(|edit| HardenRule {
                rule: edit.rule.clone(),
                start_byte: edit.original.start_byte,
                end_byte: edit.original.end_byte,
                reason: result
                    .findings
                    .iter()
                    .find(|finding| finding.rule == edit.rule)
                    .map(|finding| finding.message.clone())
                    .unwrap_or_else(|| "reviewed intentional behavior change".into()),
            })
            .collect::<Vec<_>>();
        let (_, source_path) = crate::project::resolve_entry(root, &finding.path)?;
        let permissions = file_permissions(&source_path)?;
        diffs.push(simple_diff(&finding.path, &source, &result.output));
        changes.push(HardenChange {
            path: finding.path,
            before_digest: crate::digest::sha256(source.as_bytes()),
            after_digest: crate::digest::sha256(result.output.as_bytes()),
            replacement_base64: base64::engine::general_purpose::STANDARD
                .encode(result.output.as_bytes()),
            permissions,
            rules,
        });
    }
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    blockers.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.start_byte.cmp(&right.start_byte))
            .then_with(|| left.code.cmp(&right.code))
    });
    let mut harden_plan = HardenPlan {
        schema_version: 1,
        kind: HardenKind::Harden,
        plan_digest: ZERO_DIGEST.into(),
        profile: HardenProfile::Secure,
        changes,
        validation_commands: config
            .validation_commands
            .iter()
            .map(|command| HardenCommand {
                name: command.name.clone(),
                kind: command.kind,
                argv: command.argv.clone(),
            })
            .collect(),
        blockers,
    };
    harden_plan.plan_digest = harden_plan.computed_digest()?;
    harden_plan.validate()?;
    let diff = diffs.concat();
    let approval_path = persist_plan(root, &harden_plan, &diff)?;
    Ok(PlanOutput {
        digest: harden_plan.plan_digest.clone(),
        diff,
        blockers: harden_plan.blockers.clone(),
        approval_path,
    })
}

pub(crate) fn verify(root: &Path, digest: &str) -> Result<HardenEvidence, String> {
    let (directory, plan) = load_plan(root, digest)?;
    if !plan.blockers.is_empty() {
        return Err(format!(
            "DESHELL_HARDEN_BLOCKED: {}",
            plan.blockers
                .iter()
                .map(|blocker| format!("{}: {}", blocker.code, blocker.message))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if plan.changes.is_empty() {
        return Err("DESHELL_HARDEN_NO_CHANGES: plan contains no hardening proposal".into());
    }
    let (approval, approval_digest) = load_approved(root, &plan.plan_digest)?;
    validate_current_sources(root, &plan)?;
    let config = crate::project::load_config(root).map_err(|errors| errors.join("; "))?;
    ensure_validation_unchanged(&config, &plan)?;
    let workspace = crate::workspace::private_snapshot(root)?;
    apply_changes(workspace.path(), &plan)?;
    let scanned = crate::project::scan(workspace.path())?;
    if !scanned.errors.is_empty() || !scanned.skipped.is_empty() {
        return Err("DESHELL_HARDEN_REPLACEMENT_INVALID: replacement scan is incomplete".into());
    }
    let mut validation = Vec::new();
    for command in &plan.validation_commands {
        let outcome = execute_validation(workspace.path(), command, config.limits)?;
        validation.push(HardenValidation {
            name: command.name.clone(),
            kind: command.kind,
            argv: command.argv.clone(),
            exit_code: outcome.exit_code,
            signal: outcome.signal,
            timed_out: outcome.timed_out,
            limit_exceeded: outcome.limit_exceeded,
            stdout_digest: crate::digest::sha256(&outcome.stdout),
            stderr_digest: crate::digest::sha256(&outcome.stderr),
        });
    }
    let status = if validation.iter().all(|result| {
        result.exit_code == 0
            && result.signal.is_none()
            && !result.timed_out
            && result.limit_exceeded.is_none()
    }) {
        HardenEvidenceStatus::Verified
    } else {
        HardenEvidenceStatus::Failed
    };
    let mut evidence = HardenEvidence {
        schema_version: 1,
        kind: HardenKind::Harden,
        evidence_digest: ZERO_DIGEST.into(),
        plan_digest: plan.plan_digest.clone(),
        approval_digest,
        platform_fingerprint: platform_fingerprint(),
        status,
        changes: plan
            .changes
            .iter()
            .map(|change| HardenEvidenceChange {
                path: change.path.clone(),
                before_digest: change.before_digest.clone(),
                after_digest: change.after_digest.clone(),
                rules: change.rules.iter().map(|rule| rule.rule.clone()).collect(),
            })
            .collect(),
        validation,
    };
    let _ = approval;
    evidence.evidence_digest = evidence.computed_digest()?;
    evidence.validate()?;
    persist_evidence(&directory, &evidence)?;
    Ok(evidence)
}

pub(crate) fn apply(root: &Path, digest: &str) -> Result<(), String> {
    let (directory, plan) = load_plan(root, digest)?;
    if !plan.blockers.is_empty() || plan.changes.is_empty() {
        return Err("DESHELL_HARDEN_BLOCKED: selected hardening plan is not applicable".into());
    }
    let (_, approval_digest) = load_approved(root, &plan.plan_digest)?;
    let evidence_path = directory.join("evidence.json");
    let evidence: HardenEvidence = crate::strict_json::decode(
        &std::fs::read(&evidence_path)
            .map_err(|error| format!("cannot read hardening Evidence: {error}"))?,
    )?;
    evidence.validate()?;
    if evidence.status != HardenEvidenceStatus::Verified
        || evidence.plan_digest != plan.plan_digest
        || evidence.approval_digest != approval_digest
        || evidence.platform_fingerprint != platform_fingerprint()
    {
        return Err(
            "DESHELL_HARDEN_EVIDENCE_STALE: Evidence does not match current approval and platform"
                .into(),
        );
    }
    let config = crate::project::load_config(root).map_err(|errors| errors.join("; "))?;
    ensure_validation_unchanged(&config, &plan)?;
    validate_current_sources(root, &plan)?;

    let marker_path = directory.join("applied.json");
    if marker_path.exists() {
        return Err(
            "DESHELL_HARDEN_ALREADY_APPLIED: hardening plan already has an apply marker".into(),
        );
    }
    let applied = HardenApplied {
        schema_version: 1,
        kind: HardenKind::Harden,
        plan_digest: plan.plan_digest.clone(),
        approval_digest,
        evidence_digest: evidence.evidence_digest.clone(),
        changes: evidence.changes.clone(),
    };
    let marker = encode_pretty(&applied)?;
    let mut patches = Vec::new();
    for change in &plan.changes {
        let (_, path) = crate::project::resolve_entry(root, &change.path)?;
        patches.push(crate::patch::prepare_expected(
            &path,
            &change.before_digest,
            change.replacement()?,
        )?);
    }
    patches.push(crate::patch::prepare_create(&marker_path, marker, 0o644)?);
    crate::patch::apply_all(&patches)?;
    for change in &plan.changes {
        let (_, path) = crate::project::resolve_entry(root, &change.path)?;
        let current =
            crate::digest::sha256(&std::fs::read(&path).map_err(|error| {
                format!("cannot read hardened source {}: {error}", change.path)
            })?);
        if current != change.after_digest {
            return Err(format!(
                "DESHELL_HARDEN_POST_APPLY_FAILED: {} digest mismatch",
                change.path
            ));
        }
    }
    Ok(())
}

impl HardenPlan {
    fn computed_digest(&self) -> Result<String, String> {
        let mut unsigned = self.clone();
        unsigned.plan_digest = ZERO_DIGEST.into();
        canonical_digest(&unsigned)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.kind != HardenKind::Harden {
            return Err("hardening plan version or kind is invalid".into());
        }
        if self.computed_digest()? != self.plan_digest {
            return Err("hardening plan digest mismatch".into());
        }
        let mut paths = BTreeSet::new();
        for change in &self.changes {
            let normalized = crate::ir::normalize_path(&change.path)?;
            if normalized != change.path || !paths.insert(change.path.as_str()) {
                return Err("hardening plan paths must be normalized and unique".into());
            }
            if !crate::digest::valid_sha256(&change.before_digest)
                || !crate::digest::valid_sha256(&change.after_digest)
                || change.permissions > 0o777
                || change.rules.is_empty()
            {
                return Err("hardening change digest, permissions, or rules are invalid".into());
            }
            let replacement = change.replacement()?;
            if replacement.len() > 4 * 1024 * 1024
                || crate::digest::sha256(&replacement) != change.after_digest
            {
                return Err("hardening replacement digest or size is invalid".into());
            }
            for rule in &change.rules {
                if rule.rule.trim().is_empty()
                    || rule.reason.trim().is_empty()
                    || rule.start_byte > rule.end_byte
                {
                    return Err("hardening rule is invalid".into());
                }
            }
        }
        for command in &self.validation_commands {
            validate_argv(&command.argv)?;
            if command.name.trim().is_empty() {
                return Err("hardening validation command name is empty".into());
            }
        }
        Ok(())
    }
}

impl HardenChange {
    fn replacement(&self) -> Result<Vec<u8>, String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.replacement_base64)
            .map_err(|error| format!("hardening replacement is invalid base64: {error}"))?;
        if base64::engine::general_purpose::STANDARD.encode(&bytes) != self.replacement_base64 {
            return Err("hardening replacement base64 is not canonical".into());
        }
        Ok(bytes)
    }
}

impl HardenEvidence {
    fn computed_digest(&self) -> Result<String, String> {
        let mut unsigned = self.clone();
        unsigned.evidence_digest = ZERO_DIGEST.into();
        canonical_digest(&unsigned)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.kind != HardenKind::Harden
            || self.computed_digest()? != self.evidence_digest
            || !crate::digest::valid_sha256(&self.plan_digest)
            || !crate::digest::valid_sha256(&self.approval_digest)
            || !crate::digest::valid_sha256(&self.platform_fingerprint)
        {
            return Err("hardening Evidence version, kind, or digest is invalid".into());
        }
        if self.changes.iter().any(|change| {
            crate::ir::normalize_path(&change.path).as_deref() != Ok(change.path.as_str())
                || !crate::digest::valid_sha256(&change.before_digest)
                || !crate::digest::valid_sha256(&change.after_digest)
                || change.rules.is_empty()
        }) {
            return Err("hardening Evidence change is invalid".into());
        }
        for validation in &self.validation {
            validate_argv(&validation.argv)?;
            if !crate::digest::valid_sha256(&validation.stdout_digest)
                || !crate::digest::valid_sha256(&validation.stderr_digest)
            {
                return Err("hardening Evidence output digest is invalid".into());
            }
        }
        Ok(())
    }
}

fn persist_plan(root: &Path, plan: &HardenPlan, diff: &str) -> Result<PathBuf, String> {
    let hardening = ensure_project_child(root, ".deshell", "hardening")?;
    let sha256 = ensure_child(&hardening, "sha256")?;
    let directory = ensure_child(&sha256, &plan.plan_digest)?;
    write_content_addressed(&directory.join("plan.json"), encode_pretty(plan)?)?;
    write_content_addressed(&directory.join("diff.patch"), diff.as_bytes().to_vec())?;
    let approvals = ensure_child(&hardening, "approvals")?;
    let approval_path = approvals.join(format!("{}.json", plan.plan_digest));
    if !approval_path.exists() {
        let draft = HardenApproval {
            schema_version: 1,
            plan_digest: plan.plan_digest.clone(),
            approval: HardenApprovalState::Draft,
            owner: None,
            reason: None,
        };
        crate::patch::apply_all(&[crate::patch::prepare_create(
            &approval_path,
            encode_pretty(&draft)?,
            0o644,
        )?])?;
    }
    Ok(approval_path)
}

fn load_plan(root: &Path, digest: &str) -> Result<(PathBuf, HardenPlan), String> {
    if !crate::digest::valid_sha256(digest) {
        return Err("hardening plan selector must be a SHA-256 digest".into());
    }
    let relative = format!(".deshell/hardening/sha256/{digest}/plan.json");
    let path = crate::project::project_file_path(root, &relative)?;
    let plan: HardenPlan = crate::strict_json::decode(
        &std::fs::read(&path)
            .map_err(|error| format!("cannot read selected hardening plan: {error}"))?,
    )?;
    plan.validate()?;
    if plan.plan_digest != digest {
        return Err("hardening plan filename and content digest differ".into());
    }
    Ok((
        path.parent()
            .ok_or("hardening plan has no directory")?
            .to_path_buf(),
        plan,
    ))
}

fn load_approved(root: &Path, plan_digest: &str) -> Result<(HardenApproval, String), String> {
    let path = crate::project::project_file_path(
        root,
        &format!(".deshell/hardening/approvals/{plan_digest}.json"),
    )?;
    let approval: HardenApproval = crate::strict_json::decode(
        &std::fs::read(&path)
            .map_err(|error| format!("cannot read hardening approval: {error}"))?,
    )?;
    if approval.schema_version != 1
        || approval.plan_digest != plan_digest
        || approval.approval != HardenApprovalState::Approved
        || approval
            .owner
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        || approval
            .reason
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(
            "DESHELL_HARDEN_APPROVAL_REQUIRED: edit the plan's separate approval file with an owner and reason"
                .into(),
        );
    }
    let digest = canonical_digest(&approval)?;
    Ok((approval, digest))
}

fn persist_evidence(directory: &Path, evidence: &HardenEvidence) -> Result<(), String> {
    let path = directory.join("evidence.json");
    let bytes = encode_pretty(evidence)?;
    let patch = match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            let current = std::fs::read(&path)
                .map_err(|error| format!("cannot read existing hardening Evidence: {error}"))?;
            crate::patch::prepare_expected(&path, &crate::digest::sha256(&current), bytes)?
        }
        Ok(_) => return Err("hardening Evidence path is not a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::patch::prepare_create(&path, bytes, 0o644)?
        }
        Err(error) => return Err(format!("cannot inspect hardening Evidence: {error}")),
    };
    crate::patch::apply_all(&[patch])
}

fn validate_current_sources(root: &Path, plan: &HardenPlan) -> Result<(), String> {
    for change in &plan.changes {
        let (_, path) = crate::project::resolve_entry(root, &change.path)?;
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read hardening source {}: {error}", change.path))?;
        if crate::digest::sha256(&bytes) != change.before_digest {
            return Err(format!(
                "DESHELL_HARDEN_STALE_SOURCE: {} changed after planning",
                change.path
            ));
        }
    }
    Ok(())
}

fn ensure_validation_unchanged(
    config: &crate::config::ProjectConfig,
    plan: &HardenPlan,
) -> Result<(), String> {
    let current = config
        .validation_commands
        .iter()
        .map(|command| HardenCommand {
            name: command.name.clone(),
            kind: command.kind,
            argv: command.argv.clone(),
        })
        .collect::<Vec<_>>();
    if current != plan.validation_commands {
        return Err("DESHELL_HARDEN_STALE_VALIDATION: validation commands changed".into());
    }
    Ok(())
}

fn apply_changes(root: &Path, plan: &HardenPlan) -> Result<(), String> {
    let mut patches = Vec::new();
    for change in &plan.changes {
        let (_, path) = crate::project::resolve_entry(root, &change.path)?;
        patches.push(crate::patch::prepare_expected(
            &path,
            &change.before_digest,
            change.replacement()?,
        )?);
    }
    crate::patch::apply_all(&patches)
}

fn execute_validation(
    root: &Path,
    command: &HardenCommand,
    limits: crate::config::ResourceLimits,
) -> Result<crate::agent_process::Outcome, String> {
    validate_argv(&command.argv)?;
    let mut argv = command.argv.clone();
    if argv[0].contains('/') && !Path::new(&argv[0]).is_absolute() {
        argv[0] = root.join(&argv[0]).to_string_lossy().into_owned();
    }
    crate::agent_process::execute(
        root,
        crate::agent_process::Request {
            argv,
            environment: Vec::new(),
            working_directory: None,
            stdin: Vec::new(),
            limits: limits.into(),
        },
    )
}

fn validate_argv(argv: &[String]) -> Result<(), String> {
    if argv.is_empty() || argv[0].is_empty() || argv[0].starts_with('-') {
        return Err("hardening validation requires exact argv with a non-option program".into());
    }
    if argv.iter().any(|argument| argument.contains('\0')) {
        return Err("hardening validation argv contains NUL".into());
    }
    Ok(())
}

fn platform_fingerprint() -> String {
    crate::digest::sha256(
        format!(
            "deshell-harden-platform-v1:{}:{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
        .as_bytes(),
    )
}

fn ensure_project_child(root: &Path, parent: &str, child: &str) -> Result<PathBuf, String> {
    let parent = crate::project::project_directory_path(root, parent)?;
    ensure_child(&parent, child)
}

fn ensure_child(parent: &Path, child: &str) -> Result<PathBuf, String> {
    if child.is_empty() || child == "." || child == ".." || child.contains(['/', '\\', '\0']) {
        return Err("invalid hardening directory component".into());
    }
    let path = parent.join(child);
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(format!(
                "hardening path is not a regular directory: {}",
                path.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&path)
            .map_err(|error| {
                format!(
                    "cannot create hardening directory {}: {error}",
                    path.display()
                )
            })?,
        Err(error) => {
            return Err(format!(
                "cannot inspect hardening directory {}: {error}",
                path.display()
            ));
        }
    }
    Ok(path)
}

fn write_content_addressed(path: &Path, bytes: Vec<u8>) -> Result<(), String> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            let current = std::fs::read(path).map_err(|error| {
                format!("cannot read hardening artifact {}: {error}", path.display())
            })?;
            if current == bytes {
                Ok(())
            } else {
                Err(format!(
                    "content-addressed hardening artifact differs: {}",
                    path.display()
                ))
            }
        }
        Ok(_) => Err(format!("hardening artifact is unsafe: {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::patch::apply_all(&[crate::patch::prepare_create(path, bytes, 0o644)?])
        }
        Err(error) => Err(format!(
            "cannot inspect hardening artifact {}: {error}",
            path.display()
        )),
    }
}

fn canonical_digest<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    Ok(crate::digest::sha256(
        &crate::canonical_json::canonical_bytes(&value)?,
    ))
}

fn encode_pretty<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    crate::canonical_json::pretty_bytes(&value)
}

fn file_permissions(path: &Path) -> Result<u32, String> {
    let metadata = path.symlink_metadata().map_err(|error| {
        format!(
            "cannot inspect hardening source {}: {error}",
            path.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        Ok(metadata.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Ok(0o644)
    }
}

fn simple_diff(path: &str, before: &str, after: &str) -> String {
    let mut output = format!("--- a/{path}\n+++ b/{path}\n");
    let before_lines = before.split_inclusive('\n').collect::<Vec<_>>();
    let after_lines = after.split_inclusive('\n').collect::<Vec<_>>();
    let prefix = before_lines
        .iter()
        .zip(&after_lines)
        .take_while(|(left, right)| left == right)
        .count();
    for line in &before_lines[..prefix] {
        output.push(' ');
        output.push_str(line);
    }
    for line in &before_lines[prefix..] {
        output.push('-');
        output.push_str(line);
    }
    for line in &after_lines[prefix..] {
        output.push('+');
        output.push_str(line);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_plan() -> HardenPlan {
        let mut plan = HardenPlan {
            schema_version: 1,
            kind: HardenKind::Harden,
            plan_digest: ZERO_DIGEST.into(),
            profile: HardenProfile::Secure,
            changes: vec![HardenChange {
                path: "build.sh".into(),
                before_digest: crate::digest::sha256(b"before"),
                after_digest: crate::digest::sha256(b"after"),
                replacement_base64: base64::engine::general_purpose::STANDARD.encode(b"after"),
                permissions: 0o644,
                rules: vec![HardenRule {
                    rule: "secure.strict-mode".into(),
                    start_byte: 0,
                    end_byte: 0,
                    reason: "reviewed".into(),
                }],
            }],
            validation_commands: vec![HardenCommand {
                name: "test".into(),
                kind: crate::config::ValidationKind::Test,
                argv: vec!["program".into()],
            }],
            blockers: Vec::new(),
        };
        plan.plan_digest = plan.computed_digest().unwrap();
        plan
    }

    fn resign(plan: &mut HardenPlan) {
        plan.plan_digest = plan.computed_digest().unwrap();
    }

    fn signed_evidence() -> HardenEvidence {
        let plan = signed_plan();
        let mut evidence = HardenEvidence {
            schema_version: 1,
            kind: HardenKind::Harden,
            evidence_digest: ZERO_DIGEST.into(),
            plan_digest: plan.plan_digest,
            approval_digest: crate::digest::sha256(b"approval"),
            platform_fingerprint: platform_fingerprint(),
            status: HardenEvidenceStatus::Verified,
            changes: vec![HardenEvidenceChange {
                path: "build.sh".into(),
                before_digest: crate::digest::sha256(b"before"),
                after_digest: crate::digest::sha256(b"after"),
                rules: vec!["secure.strict-mode".into()],
            }],
            validation: vec![HardenValidation {
                name: "test".into(),
                kind: crate::config::ValidationKind::Test,
                argv: vec!["program".into()],
                exit_code: 0,
                signal: None,
                timed_out: false,
                limit_exceeded: None,
                stdout_digest: crate::digest::sha256(b""),
                stderr_digest: crate::digest::sha256(b""),
            }],
        };
        evidence.evidence_digest = evidence.computed_digest().unwrap();
        evidence
    }

    fn resign_evidence(evidence: &mut HardenEvidence) {
        evidence.evidence_digest = evidence.computed_digest().unwrap();
    }

    #[test]
    fn harden_plan_and_evidence_digests_are_self_authenticating() {
        let mut plan = signed_plan();
        assert!(plan.validate().is_ok());
        plan.changes[0].permissions = 0o777;
        assert!(plan.validate().unwrap_err().contains("digest mismatch"));

        let mut evidence = signed_evidence();
        assert!(evidence.validate().is_ok());
        evidence.status = HardenEvidenceStatus::Failed;
        assert!(evidence.validate().unwrap_err().contains("digest"));
    }

    #[test]
    fn hardening_plan_validation_rejects_every_unbound_field() {
        let mut cases: Vec<(HardenPlan, &str)> = Vec::new();

        let mut plan = signed_plan();
        plan.schema_version = 2;
        resign(&mut plan);
        cases.push((plan, "version or kind"));

        let mut plan = signed_plan();
        plan.changes.push(plan.changes[0].clone());
        resign(&mut plan);
        cases.push((plan, "normalized and unique"));

        let mut plan = signed_plan();
        plan.changes[0].path = "../outside".into();
        resign(&mut plan);
        cases.push((plan, "path is not normalized"));

        for mutation in 0..4 {
            let mut plan = signed_plan();
            match mutation {
                0 => plan.changes[0].before_digest = "bad".into(),
                1 => plan.changes[0].permissions = 0o1000,
                2 => plan.changes[0].rules.clear(),
                _ => plan.changes[0].after_digest = crate::digest::sha256(b"different"),
            }
            resign(&mut plan);
            cases.push((
                plan,
                if mutation == 3 {
                    "replacement digest"
                } else {
                    "digest, permissions, or rules"
                },
            ));
        }

        let mut plan = signed_plan();
        plan.changes[0].replacement_base64 = "not-base64".into();
        resign(&mut plan);
        cases.push((plan, "invalid base64"));

        for mutation in 0..3 {
            let mut plan = signed_plan();
            match mutation {
                0 => plan.changes[0].rules[0].rule.clear(),
                1 => plan.changes[0].rules[0].reason.clear(),
                _ => {
                    plan.changes[0].rules[0].start_byte = 2;
                    plan.changes[0].rules[0].end_byte = 1;
                }
            }
            resign(&mut plan);
            cases.push((plan, "hardening rule is invalid"));
        }

        for argv in [vec![], vec!["-option".into()], vec!["program\0bad".into()]] {
            let mut plan = signed_plan();
            plan.validation_commands[0].argv = argv;
            resign(&mut plan);
            cases.push((plan, "hardening validation"));
        }
        let mut plan = signed_plan();
        plan.validation_commands[0].name.clear();
        resign(&mut plan);
        cases.push((plan, "command name is empty"));

        for (plan, expected) in cases {
            let error = plan.validate().unwrap_err();
            assert!(error.contains(expected), "missing {expected:?} in {error}");
        }
    }

    #[test]
    fn hardening_evidence_validation_rejects_invalid_changes_commands_and_digests() {
        let mut cases: Vec<(HardenEvidence, &str)> = Vec::new();
        let mut evidence = signed_evidence();
        evidence.schema_version = 2;
        resign_evidence(&mut evidence);
        cases.push((evidence, "version, kind, or digest"));

        for mutation in 0..4 {
            let mut evidence = signed_evidence();
            match mutation {
                0 => evidence.changes[0].path = "../outside".into(),
                1 => evidence.changes[0].before_digest = "bad".into(),
                2 => evidence.changes[0].after_digest = "bad".into(),
                _ => evidence.changes[0].rules.clear(),
            }
            resign_evidence(&mut evidence);
            cases.push((evidence, "Evidence change is invalid"));
        }
        let mut evidence = signed_evidence();
        evidence.validation[0].argv.clear();
        resign_evidence(&mut evidence);
        cases.push((evidence, "requires exact argv"));
        for stdout in [true, false] {
            let mut evidence = signed_evidence();
            if stdout {
                evidence.validation[0].stdout_digest = "bad".into();
            } else {
                evidence.validation[0].stderr_digest = "bad".into();
            }
            resign_evidence(&mut evidence);
            cases.push((evidence, "output digest is invalid"));
        }
        for (evidence, expected) in cases {
            let error = evidence.validate().unwrap_err();
            assert!(error.contains(expected), "missing {expected:?} in {error}");
        }
    }

    #[test]
    fn hardening_artifacts_are_content_addressed_and_directory_safe() {
        let root = tempfile::tempdir().unwrap();
        for child in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(
                ensure_child(root.path(), child).is_err(),
                "accepted {child:?}"
            );
        }
        let directory = ensure_child(root.path(), "safe").unwrap();
        assert_eq!(ensure_child(root.path(), "safe").unwrap(), directory);
        std::fs::write(root.path().join("file"), b"file").unwrap();
        assert!(
            ensure_child(root.path(), "file")
                .unwrap_err()
                .contains("not a regular")
        );

        let artifact = directory.join("artifact.json");
        write_content_addressed(&artifact, b"one".to_vec()).unwrap();
        write_content_addressed(&artifact, b"one".to_vec()).unwrap();
        assert!(
            write_content_addressed(&artifact, b"two".to_vec())
                .unwrap_err()
                .contains("differs")
        );
        std::fs::create_dir(directory.join("unsafe")).unwrap();
        assert!(
            write_content_addressed(&directory.join("unsafe"), b"value".to_vec())
                .unwrap_err()
                .contains("unsafe")
        );

        assert_eq!(platform_fingerprint().len(), 64);
        assert_eq!(
            simple_diff("a", "same\nbefore\n", "same\nafter\n"),
            "--- a/a\n+++ b/a\n same\n-before\n+after\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hardening_persistence_approval_source_and_validation_helpers_form_a_closed_cycle() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("build.sh"), b"before").unwrap();
        crate::project::init_with_entries(root.path(), &["build.sh".into()]).unwrap();

        let plan = signed_plan();
        let approval_path = persist_plan(root.path(), &plan, "diff").unwrap();
        let (directory, loaded) = load_plan(root.path(), &plan.plan_digest).unwrap();
        assert_eq!(loaded, plan);
        assert!(
            load_plan(root.path(), "bad")
                .unwrap_err()
                .contains("selector")
        );
        assert!(
            load_approved(root.path(), &plan.plan_digest)
                .unwrap_err()
                .contains("APPROVAL")
        );

        let approval = HardenApproval {
            schema_version: 1,
            plan_digest: plan.plan_digest.clone(),
            approval: HardenApprovalState::Approved,
            owner: Some("owner".into()),
            reason: Some("reviewed".into()),
        };
        std::fs::write(&approval_path, encode_pretty(&approval).unwrap()).unwrap();
        assert_eq!(
            load_approved(root.path(), &plan.plan_digest).unwrap().0,
            approval
        );
        validate_current_sources(root.path(), &plan).unwrap();
        let mut changed = plan.clone();
        changed.changes[0].before_digest = crate::digest::sha256(b"other");
        assert!(
            validate_current_sources(root.path(), &changed)
                .unwrap_err()
                .contains("STALE_SOURCE")
        );

        let config = crate::project::load_config(root.path()).unwrap();
        assert!(ensure_validation_unchanged(&config, &plan).is_err());
        let mut no_validation = plan.clone();
        no_validation.validation_commands.clear();
        ensure_validation_unchanged(&config, &no_validation).unwrap();
        apply_changes(root.path(), &plan).unwrap();
        assert_eq!(
            std::fs::read(root.path().join("build.sh")).unwrap(),
            b"after"
        );

        let evidence = signed_evidence();
        persist_evidence(&directory, &evidence).unwrap();
        persist_evidence(&directory, &evidence).unwrap();
        assert!(directory.join("evidence.json").is_file());

        let tool = root.path().join("tool");
        std::fs::write(&tool, b"#!/bin/sh\nprintf ok").unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let outcome = execute_validation(
            root.path(),
            &HardenCommand {
                name: "tool".into(),
                kind: crate::config::ValidationKind::Test,
                argv: vec!["./tool".into()],
            },
            crate::config::ResourceLimits::DEFAULT,
        )
        .unwrap();
        assert_eq!(outcome.stdout, b"ok");
    }
}
