use crate::ir::{Node, Operation, Plan, TextExpression, TextPart};
use sha2::{Digest as _, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Target {
    Internal,
    Dagger,
    Nushell,
    Cwl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Mode {
    Strict,
    Bundle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Artifact {
    pub filename: String,
    pub media_type: String,
    pub content: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BundleSource {
    File(PathBuf),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BundleFile {
    pub archive_path: String,
    pub source: BundleSource,
    pub expected_sha256: Option<String>,
    pub executable: bool,
}

pub(crate) struct BundleRequest<'a> {
    pub plan: &'a Plan,
    pub entrypoint: &'a str,
    pub target: Target,
    pub runtime_image: &'a str,
    pub runtime_assets: Vec<String>,
    pub files: Vec<BundleFile>,
}

#[derive(serde::Serialize)]
struct BundleManifest {
    schema_version: u32,
    format: &'static str,
    target: &'static str,
    entrypoint: String,
    runtime_image: String,
    operating_system: String,
    architecture: String,
    capabilities: Vec<String>,
    runtime_assets: Vec<String>,
    files: Vec<BundleManifestFile>,
    run: Vec<String>,
}

#[derive(serde::Serialize)]
struct BundleManifestFile {
    path: String,
    bytes: u64,
    sha256: String,
    executable: bool,
}

pub(crate) fn export(
    plan: &Plan,
    target: Target,
    mode: Mode,
    runtime_image: Option<&str>,
) -> Result<Artifact, String> {
    plan.validate().map_err(|errors| errors.join("; "))?;
    if mode == Mode::Bundle {
        return Err("bundle export requires a project context and --output".into());
    }
    if target == Target::Internal {
        return Ok(Artifact {
            filename: "plan.json".into(),
            media_type: "application/vnd.deshell.effect-ir+json".into(),
            content: plan.encode_pretty()?,
        });
    }
    let task = plan
        .tasks
        .iter()
        .find(|task| task.name == plan.entrypoint)
        .ok_or_else(|| format!("entrypoint task not found: {}", plan.entrypoint))?;
    let commands = if task.inputs.is_empty()
        && task.outputs.is_empty()
        && task.environment.is_empty()
        && task.secrets.is_empty()
        && task.invocation.is_none()
    {
        flatten_exec(&task.body)
    } else {
        Err(format!(
            "strict exporter cannot represent task {} interface",
            task.name
        ))
    };
    let commands = commands?;
    match target {
        Target::Internal => unreachable!(),
        Target::Dagger => {
            let image = runtime_image
                .filter(|image| crate::lab::digest_pinned(image))
                .ok_or("strict Dagger export requires a digest-pinned target runtime from deshell.lock")?;
            Ok(dagger(&commands, image))
        }
        Target::Nushell => Ok(nushell(&commands)),
        Target::Cwl if commands.len() == 1 => cwl(&commands[0]),
        Target::Cwl => {
            Err("strict CWL CommandLineTool export requires exactly one Exec node".into())
        }
    }
}

pub(crate) fn write_bundle(
    request: BundleRequest<'_>,
    writer: &mut dyn Write,
) -> Result<(), String> {
    request
        .plan
        .validate()
        .map_err(|errors| errors.join("; "))?;
    if !crate::lab::digest_pinned(request.runtime_image) {
        return Err("bundle export requires a digest-pinned runtime image".into());
    }
    if plan_has_residual(request.plan) {
        return Err("bundle export cannot execute non-executable residual nodes".into());
    }
    let entrypoint = crate::ir::normalize_path(request.entrypoint)?;
    if entrypoint != request.entrypoint {
        return Err("bundle entrypoint must be a normalized project-relative path".into());
    }
    let mut files = request.files;
    files.sort_by(|left, right| left.archive_path.cmp(&right.archive_path));
    let mut previous = None;
    let mut manifest_files = Vec::with_capacity(files.len());
    for file in &files {
        let normalized = crate::ir::normalize_path(&file.archive_path)?;
        if normalized != file.archive_path {
            return Err(format!(
                "bundle archive path is not normalized: {}",
                file.archive_path
            ));
        }
        if previous.is_some_and(|value: &str| value == file.archive_path) {
            return Err(format!(
                "duplicate bundle archive path: {}",
                file.archive_path
            ));
        }
        previous = Some(file.archive_path.as_str());
        let (bytes, sha256) = inspect_bundle_source(&file.source)?;
        if let Some(expected) = &file.expected_sha256 {
            let expected = expected.strip_prefix("sha256:").unwrap_or(expected);
            if expected != sha256 {
                return Err(format!(
                    "bundle source digest mismatch for {}: expected {expected}, found {sha256}",
                    file.archive_path
                ));
            }
        }
        manifest_files.push(BundleManifestFile {
            path: file.archive_path.clone(),
            bytes,
            sha256,
            executable: file.executable,
        });
    }
    let binary = manifest_files
        .iter()
        .find(|file| file.path == "bin/deshell" || file.path == "bin/deshell.exe")
        .ok_or("bundle export requires the exact deshell binary")?;
    if !binary.executable {
        return Err("bundled deshell binary must be executable".into());
    }
    let mut runtime_assets = request.runtime_assets;
    runtime_assets.sort();
    let runtime_asset_count = runtime_assets.len();
    runtime_assets.dedup();
    if runtime_assets.len() != runtime_asset_count {
        return Err("bundle runtime asset paths must be unique".into());
    }
    if runtime_assets.is_empty() {
        return Err("bundle export requires at least one locked runtime asset".into());
    }
    for asset in &runtime_assets {
        let normalized = crate::ir::normalize_path(asset)?;
        if normalized != *asset {
            return Err(format!(
                "bundle runtime asset path is not normalized: {asset}"
            ));
        }
        if !manifest_files.iter().any(|file| &file.path == asset) {
            return Err(format!(
                "bundle runtime asset is absent from the archive: {asset}"
            ));
        }
    }
    let manifest = BundleManifest {
        schema_version: 1,
        format: "deshell-bundle-v1",
        target: target_name(request.target),
        entrypoint: entrypoint.clone(),
        runtime_image: request.runtime_image.into(),
        operating_system: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        capabilities: plan_capabilities(request.plan),
        runtime_assets,
        files: manifest_files,
        run: vec![
            if cfg!(windows) {
                "bin/deshell.exe".into()
            } else {
                "bin/deshell".into()
            },
            "run".into(),
            "--root".into(),
            "project".into(),
            "--entry".into(),
            entrypoint,
        ],
    };
    let manifest = crate::canonical_json::pretty_bytes(
        &serde_json::to_value(manifest).map_err(|error| error.to_string())?,
    )?;
    let manifest_file = BundleFile {
        archive_path: "bundle-manifest.json".into(),
        source: BundleSource::Bytes(manifest),
        expected_sha256: None,
        executable: false,
    };
    files.push(manifest_file);
    files.sort_by(|left, right| left.archive_path.cmp(&right.archive_path));
    for file in &files {
        write_tar_file(writer, file)?;
    }
    writer
        .write_all(&[0_u8; 1024])
        .map_err(|error| format!("cannot finish bundle archive: {error}"))?;
    writer
        .flush()
        .map_err(|error| format!("cannot flush bundle archive: {error}"))
}

fn target_name(target: Target) -> &'static str {
    match target {
        Target::Internal => "internal",
        Target::Dagger => "dagger",
        Target::Nushell => "nushell",
        Target::Cwl => "cwl",
    }
}

fn plan_has_residual(plan: &Plan) -> bool {
    fn node_has_residual(node: &Node) -> bool {
        if matches!(node.guarantee, crate::ir::Guarantee::Residual { .. }) {
            return true;
        }
        node_children(node).into_iter().any(node_has_residual)
    }
    plan.tasks.iter().any(|task| node_has_residual(&task.body))
}

fn plan_capabilities(plan: &Plan) -> Vec<String> {
    fn visit(node: &Node, output: &mut std::collections::BTreeSet<String>) {
        match &node.operation {
            Operation::Exec { .. } => {
                output.insert("process".into());
            }
            Operation::FileRead { .. } => {
                output.insert("file-read".into());
            }
            Operation::FileWrite { .. } | Operation::FileRemove { .. } => {
                output.insert("file-write".into());
            }
            Operation::NetworkRequest { .. } => {
                output.insert("network".into());
            }
            Operation::InterpreterCall { capabilities, .. } => {
                output.insert("delegation".into());
                output.extend(capabilities.iter().cloned());
            }
            _ => {}
        }
        for child in node_children(node) {
            visit(child, output);
        }
    }
    let mut output = std::collections::BTreeSet::new();
    for task in &plan.tasks {
        output.extend(task.platform_capabilities.iter().cloned());
        visit(&task.body, &mut output);
    }
    output.into_iter().collect()
}

fn node_children(node: &Node) -> Vec<&Node> {
    match &node.operation {
        Operation::Pipeline { nodes, .. }
        | Operation::Sequence { nodes }
        | Operation::Parallel { nodes } => nodes.iter().collect(),
        Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            let mut nodes = vec![predicate.as_ref(), if_true.as_ref()];
            nodes.extend(if_false.as_deref());
            nodes
        }
        Operation::Match { cases, default, .. } => {
            let mut nodes = cases.iter().map(|case| &case.body).collect::<Vec<_>>();
            nodes.extend(default.as_deref());
            nodes
        }
        Operation::Foreach { body, .. } | Operation::CaptureStdout { body, .. } => vec![body],
        Operation::TryFinally { body, finalizer } => vec![body, finalizer],
        _ => Vec::new(),
    }
}

fn inspect_bundle_source(source: &BundleSource) -> Result<(u64, String), String> {
    match source {
        BundleSource::Bytes(bytes) => {
            Ok((bytes.len() as u64, crate::digest::sha256(bytes.as_slice())))
        }
        BundleSource::File(path) => crate::digest::file_sha256(path)
            .map_err(|error| format!("cannot hash bundle source: {error}")),
    }
}

fn write_tar_file(writer: &mut dyn Write, file: &BundleFile) -> Result<(), String> {
    let archive_path = format!("deshell-bundle/{}", file.archive_path);
    let (size, expected_digest) = inspect_bundle_source(&file.source)?;
    let header = tar_header(
        &archive_path,
        size,
        if file.executable { 0o755 } else { 0o644 },
    )?;
    writer
        .write_all(&header)
        .map_err(|error| format!("cannot write bundle header: {error}"))?;
    let actual_digest = match &file.source {
        BundleSource::Bytes(bytes) => {
            writer
                .write_all(bytes)
                .map_err(|error| format!("cannot write bundle entry: {error}"))?;
            crate::digest::sha256(bytes)
        }
        BundleSource::File(path) => {
            let mut source = std::fs::File::open(path).map_err(|error| {
                format!("cannot reopen bundle source {}: {error}", path.display())
            })?;
            let mut remaining = size;
            let mut digest = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            while remaining != 0 {
                let limit = usize::try_from(remaining.min(buffer.len() as u64))
                    .expect("bounded bundle read size");
                let count = source.read(&mut buffer[..limit]).map_err(|error| {
                    format!("cannot read bundle source {}: {error}", path.display())
                })?;
                if count == 0 {
                    return Err(format!(
                        "bundle source shrank while writing: {}",
                        path.display()
                    ));
                }
                writer
                    .write_all(&buffer[..count])
                    .map_err(|error| format!("cannot write bundle entry: {error}"))?;
                digest.update(&buffer[..count]);
                remaining -= count as u64;
            }
            let mut extra = [0_u8; 1];
            if source.read(&mut extra).map_err(|error| {
                format!(
                    "cannot finish reading bundle source {}: {error}",
                    path.display()
                )
            })? != 0
            {
                return Err(format!(
                    "bundle source grew while writing: {}",
                    path.display()
                ));
            }
            crate::digest::lowercase_hex(digest.finalize())
        }
    };
    if actual_digest != expected_digest {
        return Err(format!(
            "bundle source changed while writing: {}",
            file.archive_path
        ));
    }
    let padding = (512 - size % 512) % 512;
    if padding != 0 {
        writer
            .write_all(&vec![0_u8; padding as usize])
            .map_err(|error| format!("cannot pad bundle entry: {error}"))?;
    }
    Ok(())
}

fn tar_header(path: &str, size: u64, mode: u32) -> Result<[u8; 512], String> {
    let (prefix, name) = split_tar_path(path)?;
    let mut header = [0_u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    write_octal(&mut header[100..108], u64::from(mode))?;
    write_octal(&mut header[108..116], 0)?;
    write_octal(&mut header[116..124], 0)?;
    write_octal(&mut header[124..136], size)?;
    write_octal(&mut header[136..148], 0)?;
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    if !prefix.is_empty() {
        header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    }
    let checksum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
    let checksum = format!("{checksum:06o}\0 ");
    if checksum.len() != 8 {
        return Err("tar header checksum overflow".into());
    }
    header[148..156].copy_from_slice(checksum.as_bytes());
    Ok(header)
}

fn split_tar_path(path: &str) -> Result<(&str, &str), String> {
    if path.len() <= 100 {
        return Ok(("", path));
    }
    for (index, _) in path.match_indices('/').rev() {
        let prefix = &path[..index];
        let name = &path[index + 1..];
        if prefix.len() <= 155 && name.len() <= 100 {
            return Ok((prefix, name));
        }
    }
    Err(format!("bundle archive path is too long for ustar: {path}"))
}

fn write_octal(field: &mut [u8], value: u64) -> Result<(), String> {
    let digits = format!("{value:o}");
    if digits.len() + 1 > field.len() {
        return Err("bundle entry is too large for ustar".into());
    }
    field.fill(b'0');
    let start = field.len() - digits.len() - 1;
    field[start..start + digits.len()].copy_from_slice(digits.as_bytes());
    field[field.len() - 1] = 0;
    Ok(())
}

fn flatten_exec(node: &Node) -> Result<Vec<Vec<String>>, String> {
    match &node.operation {
        Operation::Exec {
            argv,
            environment,
            working_directory,
        } => {
            if !environment.is_empty() {
                return Err(format!(
                    "strict exporter cannot represent node {} (exec.environment)",
                    node.id
                ));
            }
            if working_directory.is_some() {
                return Err(format!(
                    "strict exporter cannot represent node {} (exec.working_directory)",
                    node.id
                ));
            }
            let argv = argv.iter().map(literal).collect::<Result<Vec<_>, _>>()?;
            Ok(vec![argv])
        }
        Operation::Sequence { .. } => Err(format!(
            "strict exporter cannot preserve shell sequence status semantics for node {}",
            node.id
        )),
        other => Err(format!(
            "strict exporter cannot represent node {} ({})",
            node.id,
            other.name()
        )),
    }
}

fn literal(expression: &TextExpression) -> Result<String, String> {
    match expression.parts.as_slice() {
        [TextPart::Literal { value }] => Ok(value.clone()),
        _ => Err("strict exporter cannot represent a dynamic text expression".into()),
    }
}

fn dagger(commands: &[Vec<String>], image: &str) -> Artifact {
    let steps = commands.iter().map(|argv| {
        let argv = serde_json::to_string(argv).expect("argv JSON serialization");
        format!("    container = container.withExec({argv});\n    output += await container.stdout();")
    }).collect::<Vec<_>>().join("\n");
    let image = serde_json::to_string(image).expect("runtime image JSON serialization");
    let content = format!(
        concat!(
            "import {{ dag, Container, object, func }} from \"@dagger.io/dagger\";\n\n",
            "@object()\nexport class Deshell {{\n",
            "  @func()\n  async main(): Promise<string> {{\n",
            "    let container: Container = dag.container().from({});\n",
            "    let output = \"\";\n",
            "{}\n",
            "    return output;\n  }}\n}}\n"
        ),
        image, steps,
    );
    Artifact {
        filename: "deshell.dagger.ts".into(),
        media_type: "text/typescript".into(),
        content: content.into_bytes(),
    }
}

fn nushell(commands: &[Vec<String>]) -> Artifact {
    let lines = commands
        .iter()
        .map(|argv| {
            let values = argv
                .iter()
                .map(|value| serde_json::to_string(value).expect("argv string JSON"))
                .collect::<Vec<_>>()
                .join(" ");
            format!("  run-external {values}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Artifact {
        filename: "deshell.nu".into(),
        media_type: "text/x-nushell".into(),
        content: format!("export def main [] {{\n{lines}\n}}\n").into_bytes(),
    }
}

fn cwl(argv: &[String]) -> Result<Artifact, String> {
    let (executable, arguments) = argv
        .split_first()
        .ok_or("CWL command argv must not be empty")?;
    let value = serde_json::json!({
        "arguments": arguments,
        "baseCommand": [executable],
        "class": "CommandLineTool",
        "cwlVersion": "v1.2",
        "inputs": {},
        "outputs": {"stdout": {"type": "stdout"}},
        "stdout": "deshell.stdout"
    });
    Ok(Artifact {
        filename: "deshell.cwl".into(),
        media_type: "application/cwl+json".into(),
        content: crate::canonical_json::pretty_bytes(&value)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Guarantee, Node, Operation, Task, TextExpression, TextPart};

    const RUNTIME: &str = concat!(
        "ghcr.io/deshell-lang/lab@sha256:",
        "14358309a308569c32bdc37e2e0e9694be33a9d99e68afb0f5ff33cc1f695dce"
    );

    fn node(operation: Operation) -> Node {
        Node {
            id: String::new(),
            operation,
            guarantee: Guarantee::Native {
                semantic_model: "export-test-v1".into(),
            },
            source: None,
        }
    }
    fn exec(values: &[&str]) -> Node {
        node(Operation::Exec {
            argv: values
                .iter()
                .map(|value| TextExpression::literal(*value))
                .collect(),
            environment: vec![],
            working_directory: None,
        })
    }
    fn plan(body: Node) -> Plan {
        let mut plan = Plan {
            schema_version: 1,
            generator: "test".into(),
            entrypoint: "main".into(),
            tasks: vec![Task {
                name: "main".into(),
                inputs: vec![],
                outputs: vec![],
                environment: vec![],
                secrets: vec![],
                platform_capabilities: vec![],
                cacheable: false,
                invocation: None,
                body,
            }],
        };
        plan.assign_node_ids().unwrap();
        plan
    }

    #[test]
    fn internal_export_is_the_canonical_plan_bytes() {
        let plan = plan(exec(&["printf", "hello"]));
        let artifact = export(&plan, Target::Internal, Mode::Strict, None).unwrap();
        assert_eq!(artifact.filename, "plan.json");
        assert_eq!(artifact.content, plan.encode_pretty().unwrap());
    }

    #[test]
    fn strict_literal_exports_are_well_formed() {
        let plan = plan(exec(&["printf", "%s", "hello"]));
        let cwl = export(&plan, Target::Cwl, Mode::Strict, None).unwrap();
        let document: serde_json::Value = crate::strict_json::parse(&cwl.content).unwrap();
        assert_eq!(document["cwlVersion"], "v1.2");
        assert_eq!(document["baseCommand"], serde_json::json!(["printf"]));
        assert_eq!(document["arguments"], serde_json::json!(["%s", "hello"]));
        let nu = String::from_utf8(
            export(&plan, Target::Nushell, Mode::Strict, None)
                .unwrap()
                .content,
        )
        .unwrap();
        assert!(nu.starts_with("export def main [] {"));
        let dagger = String::from_utf8(
            export(&plan, Target::Dagger, Mode::Strict, Some(RUNTIME))
                .unwrap()
                .content,
        )
        .unwrap();
        assert!(dagger.contains("ghcr.io/deshell-lang/lab@sha256:"));
        assert!(dagger.contains("withExec"));
    }

    #[test]
    fn strict_export_rejects_dynamic_or_unrepresentable_nodes() {
        let dynamic = plan(node(Operation::Exec {
            argv: vec![
                TextExpression::literal("emit"),
                TextExpression {
                    parts: vec![TextPart::Variable {
                        name: "VALUE".into(),
                    }],
                },
            ],
            environment: vec![],
            working_directory: None,
        }));
        let error = export(&dynamic, Target::Nushell, Mode::Strict, None).unwrap_err();
        assert!(error.contains("dynamic text expression"), "{error}");
        let file = plan(node(Operation::FileRead {
            path: TextExpression::literal("input"),
        }));
        assert!(
            export(&file, Target::Dagger, Mode::Strict, Some(RUNTIME))
                .unwrap_err()
                .contains("file_read")
        );
    }

    #[test]
    fn generic_exporter_never_invents_a_bundle_without_project_assets() {
        let file = plan(node(Operation::FileRead {
            path: TextExpression::literal("input"),
        }));
        let error = export(&file, Target::Cwl, Mode::Bundle, None).unwrap_err();
        assert!(error.contains("bundle export"), "{error}");
    }

    #[test]
    fn project_bundle_is_deterministic_and_binds_exact_runtime_assets() {
        let plan = plan(exec(&["printf", "hello"]));
        let files = vec![
            BundleFile {
                archive_path: "bin/deshell".into(),
                source: BundleSource::Bytes(b"binary".to_vec()),
                expected_sha256: Some(crate::digest::sha256(b"binary")),
                executable: true,
            },
            BundleFile {
                archive_path: "project/deshell.lock".into(),
                source: BundleSource::Bytes(b"lock".to_vec()),
                expected_sha256: None,
                executable: false,
            },
            BundleFile {
                archive_path: "project/.deshell/runtime/lab.oci.tar".into(),
                source: BundleSource::Bytes(b"runtime".to_vec()),
                expected_sha256: Some(format!("sha256:{}", crate::digest::sha256(b"runtime"))),
                executable: false,
            },
        ];
        let make = || {
            let mut archive = Vec::new();
            write_bundle(
                BundleRequest {
                    plan: &plan,
                    entrypoint: "build.sh",
                    target: Target::Cwl,
                    runtime_image: RUNTIME,
                    runtime_assets: vec!["project/.deshell/runtime/lab.oci.tar".into()],
                    files: files.clone(),
                },
                &mut archive,
            )
            .unwrap();
            archive
        };
        let first = make();
        assert_eq!(first, make());
        let entries = tar_entries(&first);
        let manifest = crate::strict_json::parse(
            entries
                .iter()
                .find(|(name, _)| name == "deshell-bundle/bundle-manifest.json")
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(manifest["format"], "deshell-bundle-v1");
        assert_eq!(manifest["target"], "cwl");
        assert_eq!(manifest["entrypoint"], "build.sh");
        assert_eq!(manifest["runtime_image"], RUNTIME);
        assert_eq!(manifest["capabilities"], serde_json::json!(["process"]));
        assert_eq!(
            manifest["runtime_assets"],
            serde_json::json!(["project/.deshell/runtime/lab.oci.tar"])
        );
        assert_eq!(
            manifest["run"],
            serde_json::json!([
                if cfg!(windows) {
                    "bin/deshell.exe"
                } else {
                    "bin/deshell"
                },
                "run",
                "--root",
                "project",
                "--entry",
                "build.sh"
            ])
        );
        assert!(entries.iter().any(|(name, contents)| name
            == "deshell-bundle/project/.deshell/runtime/lab.oci.tar"
            && *contents == b"runtime"));
    }

    fn tar_entries(archive: &[u8]) -> Vec<(String, &[u8])> {
        let mut offset = 0;
        let mut output = Vec::new();
        while archive[offset..offset + 512].iter().any(|byte| *byte != 0) {
            let header = &archive[offset..offset + 512];
            let text = |field: &[u8]| {
                std::str::from_utf8(field)
                    .unwrap()
                    .trim_end_matches('\0')
                    .to_owned()
            };
            let name = text(&header[..100]);
            let prefix = text(&header[345..500]);
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let size = usize::from_str_radix(
                std::str::from_utf8(&header[124..136])
                    .unwrap()
                    .trim_end_matches('\0')
                    .trim_start_matches('0'),
                8,
            )
            .unwrap_or(0);
            let start = offset + 512;
            output.push((path, &archive[start..start + size]));
            offset = start + size.div_ceil(512) * 512;
        }
        output
    }

    #[test]
    fn strict_exporters_reject_sequence_status_semantics_they_cannot_preserve() {
        let sequence = plan(node(Operation::Sequence {
            nodes: vec![exec(&["one"]), exec(&["two"])],
        }));
        assert!(export(&sequence, Target::Cwl, Mode::Strict, None).is_err());
        assert!(export(&sequence, Target::Nushell, Mode::Strict, None).is_err());
        assert!(export(&sequence, Target::Dagger, Mode::Strict, Some(RUNTIME)).is_err());
    }
}
