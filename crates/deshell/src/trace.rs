//! What de-shell did, in the order it did it.
//!
//! `--diagnostics` explains a failure to somebody who hit one. It says nothing
//! about a run that succeeded, and nothing at all about the operations a
//! command performed on the way — which is precisely what is needed when the
//! question is "why did these two runs differ" or "what touched that file".
//! The approval race that blocked this repository's CI for days was a write
//! that no record named.
//!
//! Trace v1 is that record. It is off by default, it never writes to stdout,
//! and it carries no value that could be a secret: names, paths, lengths and
//! digests, never contents. That is a property of the vocabulary below rather
//! than of each call site, so it holds for a caller that has not thought about
//! it.
//!
//! The events come from the layers that are already the only route to what they
//! describe — [`crate::patch`] for every filesystem mutation and
//! [`crate::host`] for every ambient read — so a new call site is traced
//! because it compiles, not because somebody remembered.

use serde::Serialize;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether a run records what it did, and in what form.
#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub(crate) enum Mode {
    /// Record nothing. The default: a trace is a thing somebody asked for.
    Off,
    /// One JSON object per line, in the order the events happened.
    Jsonl,
}

/// One thing de-shell did.
///
/// The vocabulary is closed, and `contracts/schema/trace-v1.schema.json` names
/// the same events. `cargo xtask trace-events` holds the two equal, so an event
/// added here without a contract fails the lint rather than reaching a reader
/// who has no way to decode it.
///
/// No variant carries a value read from the environment, a file or a stream.
/// A trace that could carry a secret would have to be handled like one, and
/// then nobody would turn it on.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(crate) enum Event {
    /// A directory was established, and whether this call is what created it.
    DirectoryCreate { path: String, created: bool },
    /// Bytes were staged beside their destination, before any commit.
    FileStage { path: String, bytes: u64 },
    /// A staged file replaced its destination. This is the rename.
    FileCommit { path: String, digest: String },
    /// A file was removed.
    FileRemove { path: String },
    /// A commit was undone and the destination restored.
    FileRollback { path: String },
    /// An environment variable was read. The name and whether it was set —
    /// never the value, which is where a token lives.
    EnvironmentRead { name: String, present: bool },
    /// The wall clock was read. What it said is not recorded: it is the one
    /// reading guaranteed to differ between two identical runs.
    ClockRead,
    /// A digest was computed over some bytes.
    Digest { bytes: u64, digest: String },
    /// A process was started, with the exact argv it was given.
    ProcessStart { program: String, argv: Vec<String> },
    /// A process ended. `code` is absent when a signal ended it.
    ProcessExit { program: String, code: Option<i32> },
    /// A review was decided: what it is about, the digest of the thing
    /// reviewed, and whether the review stands. A stale approval and a missing
    /// one look the same from outside and mean different things.
    ApprovalDecision {
        subject: String,
        digest: String,
        status: String,
    },
    /// A disposable provider was chosen, or refused. `provider` is absent when
    /// this platform has none available.
    ProviderSelect {
        platform: String,
        provider: Option<String>,
    },
}

impl Event {
    /// Every event, in the order the contract lists them.
    ///
    /// Beside the enum so the two are read together, and `#[cfg(test)]` because
    /// nothing constructs an event from a name at run time: the comparison with
    /// the schema is the only thing that needs the list. `cargo xtask
    /// trace-events` reads it as text, so it is checked in a release build too.
    #[cfg(test)]
    pub(crate) const NAMES: [&'static str; 12] = [
        "directory_create",
        "file_stage",
        "file_commit",
        "file_remove",
        "file_rollback",
        "environment_read",
        "clock_read",
        "digest",
        "process_start",
        "process_exit",
        "approval_decision",
        "provider_select",
    ];
}

struct Recorder {
    sink: Sink,
    started: crate::host::Stopwatch,
    /// The thread that installed this recorder.
    ///
    /// A run has one recorder and a process has one run, so in a release build
    /// every thread's work belongs to it and there is nothing to own. A test
    /// binary is the other case: the harness runs many tests in one process and
    /// in parallel, so a recorder installed by one of them would otherwise hold
    /// every digest, write and launch the others performed at the same time.
    /// The trace read back would be a record of the suite rather than of the
    /// work under test — and it would differ from run to run, which is the
    /// opposite of what the determinism comparison is checking.
    #[cfg(test)]
    owner: std::thread::ThreadId,
}

enum Sink {
    File(std::fs::File),
    StandardError(std::io::Stderr),
    #[cfg(test)]
    Shared(testing::Shared),
}

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::File(file) => file.write(bytes),
            Self::StandardError(stderr) => stderr.write(bytes),
            #[cfg(test)]
            Self::Shared(shared) => shared.write(bytes),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::File(file) => file.flush(),
            Self::StandardError(stderr) => stderr.flush(),
            #[cfg(test)]
            Self::Shared(shared) => shared.flush(),
        }
    }
}

static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
/// Whether anything is recording.
///
/// Read before an event is built, so a run with no trace — every run that did
/// not ask for one — pays one relaxed load and does not format a path, clone a
/// digest or collect an argv.
static RECORDING: AtomicBool = AtomicBool::new(false);

fn start(sink: Sink) {
    SEQUENCE.store(0, Ordering::SeqCst);
    let recorder = Recorder {
        sink,
        started: crate::host::Stopwatch::start(),
        #[cfg(test)]
        owner: std::thread::current().id(),
    };
    if let Ok(mut held) = RECORDER.lock() {
        *held = Some(recorder);
        RECORDING.store(true, Ordering::SeqCst);
    }
}

/// Begin recording to a file selected by the caller.
pub(crate) fn start_file(file: std::fs::File) {
    start(Sink::File(file));
}

/// Begin recording to standard error, leaving standard output unchanged.
pub(crate) fn start_standard_error() {
    start(Sink::StandardError(std::io::stderr()));
}

#[cfg(test)]
fn start_shared(shared: testing::Shared) {
    start(Sink::Shared(shared));
}

/// Stop recording and release the sink, flushing what is held.
pub(crate) fn stop() {
    if let Ok(mut held) = RECORDER.lock() {
        RECORDING.store(false, Ordering::SeqCst);
        if let Some(mut recorder) = held.take() {
            let _flush_result = recorder.sink.flush();
        }
    }
}

/// The record written for one event.
#[derive(Serialize)]
struct Record<'a> {
    schema_version: u32,
    sequence: u64,
    /// How long into the run the event happened. The one field that differs
    /// between two identical runs, named so a comparison can say so.
    elapsed_nanos: u128,
    #[serde(flatten)]
    event: &'a Event,
}

/// Record that this happened.
///
/// The event is a closure so that a run with no trace does not build one: these
/// sit in the filesystem, digest and launch paths, which are the paths that run
/// most. A failed write is dropped — a trace is a record of the work, never a
/// reason the work fails.
pub(crate) fn record(event: impl FnOnce() -> Event) {
    if !RECORDING.load(Ordering::Relaxed) {
        return;
    }
    let Ok(mut held) = RECORDER.lock() else {
        return;
    };
    let Some(recorder) = held.as_mut() else {
        return;
    };
    #[cfg(test)]
    if recorder.owner != std::thread::current().id() {
        return;
    }
    let event = &event();
    let record = Record {
        schema_version: 1,
        sequence: SEQUENCE.fetch_add(1, Ordering::SeqCst),
        elapsed_nanos: recorder.started.elapsed().as_nanos(),
        event,
    };
    let Ok(mut line) = serde_json::to_vec(&record) else {
        return;
    };
    line.push(b'\n');
    let _write_result = recorder.sink.write_all(&line);
}

/// A path as the trace names it.
///
/// Canonical where the path can be resolved, because the transactional layer
/// canonicalizes a patch target and does not canonicalize a directory it is
/// about to create — so the same tree appeared in one trace as both `/tmp/x`
/// and `/private/tmp/x`, and a reader had no way to know those were one place.
pub(crate) fn path_name(path: &std::path::Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Recording a trace, for a test that wants to read one back.
///
/// The recorder is one per process, because the work it records happens on more
/// than one thread and a thread-local one would miss most of it. That makes it
/// shared state between tests running in parallel, so they take turns here
/// rather than each inventing its own lock and missing the others.
#[cfg(test)]
pub(crate) mod testing {
    use super::{Mutex, start_shared, stop};
    use std::io::Write;
    use std::sync::Arc;

    static TURN: Mutex<()> = Mutex::new(());

    #[derive(Default)]
    struct SharedState {
        bytes: Vec<u8>,
        flushes: usize,
    }

    #[derive(Clone, Default)]
    pub(crate) struct Shared(Arc<Mutex<SharedState>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(|held| held.into_inner())
                .bytes
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0
                .lock()
                .unwrap_or_else(|held| held.into_inner())
                .flushes += 1;
            Ok(())
        }
    }

    impl Shared {
        pub(crate) fn text(&self) -> String {
            String::from_utf8(
                self.0
                    .lock()
                    .unwrap_or_else(|held| held.into_inner())
                    .bytes
                    .clone(),
            )
            .expect("a trace is UTF-8")
        }

        pub(crate) fn flushes(&self) -> usize {
            self.0
                .lock()
                .unwrap_or_else(|held| held.into_inner())
                .flushes
        }
    }

    /// Run work while no other tracing test can replace the process recorder.
    pub(crate) fn serialized<T>(body: impl FnOnce() -> T) -> T {
        let _turn = TURN.lock().unwrap_or_else(|held| held.into_inner());
        body()
    }

    /// Run `body` with a recorder installed, and return what it wrote.
    pub(crate) fn recorded(body: impl FnOnce()) -> String {
        held(body).1
    }

    /// The same, keeping whatever `body` returned.
    pub(crate) fn held<T>(body: impl FnOnce() -> T) -> (T, String) {
        // A test that fails while holding the lock poisons it, and the other
        // tracing tests would then fail for a reason that is not theirs.
        serialized(|| {
            let sink = Shared::default();
            start_shared(sink.clone());
            let answer = body();
            stop();
            (answer, sink.text())
        })
    }

    /// Every record the trace holds, in order.
    pub(crate) fn events(text: &str) -> Vec<serde_json::Value> {
        text.lines()
            .map(|line| serde_json::from_str(line).expect("a trace line is JSON"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::testing::{Shared, recorded, serialized};

    #[test]
    fn stopping_flushes_the_selected_sink() {
        serialized(|| {
            let sink = Shared::default();
            start_shared(sink.clone());
            assert_eq!(sink.flushes(), 0);
            stop();
            assert_eq!(sink.flushes(), 1);
        });
    }

    #[test]
    fn a_file_sink_receives_records() {
        serialized(|| {
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join("trace.jsonl");
            start_file(std::fs::File::create(&destination).unwrap());
            record(|| Event::ClockRead);
            stop();

            let text = std::fs::read_to_string(destination).unwrap();
            let events = super::testing::events(&text);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["event"], "clock_read");
        });
    }

    #[test]
    fn standard_error_is_the_selected_sink() {
        serialized(|| {
            stop();
            start_standard_error();
            let selected = RECORDER
                .lock()
                .unwrap_or_else(|held| held.into_inner())
                .as_ref()
                .is_some_and(|recorder| matches!(&recorder.sink, Sink::StandardError(_)));
            stop();
            assert!(selected);
        });
    }

    #[test]
    fn path_names_canonicalize_existing_paths_and_preserve_missing_paths() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            path_name(directory.path()),
            directory.path().canonicalize().unwrap().to_string_lossy()
        );

        let missing = directory.path().join("not-created");
        assert_eq!(path_name(&missing), missing.to_string_lossy());
    }

    /// Every event round-trips as one line, and the vocabulary is the one the
    /// enum declares.
    #[test]
    fn a_trace_is_one_json_line_per_event_over_a_closed_vocabulary() {
        let every = [
            Event::DirectoryCreate {
                path: ".deshell".into(),
                created: true,
            },
            Event::FileStage {
                path: ".deshell/project.toml".into(),
                bytes: 12,
            },
            Event::FileCommit {
                path: ".deshell/project.toml".into(),
                digest: "sha256:aa".into(),
            },
            Event::FileRemove {
                path: "build.sh".into(),
            },
            Event::FileRollback {
                path: "build.sh".into(),
            },
            Event::EnvironmentRead {
                name: "PATH".into(),
                present: true,
            },
            Event::ClockRead,
            Event::Digest {
                bytes: 4,
                digest: "sha256:bb".into(),
            },
            Event::ProcessStart {
                program: "/bin/sh".into(),
                argv: vec!["-c".into(), "printf x".into()],
            },
            Event::ProcessExit {
                program: "/bin/sh".into(),
                code: Some(0),
            },
            Event::ApprovalDecision {
                subject: "scenario".into(),
                digest: "sha256:dd".into(),
                status: "stale".into(),
            },
            Event::ProviderSelect {
                platform: "macos".into(),
                provider: None,
            },
        ];
        let text = recorded(|| {
            for event in every.clone() {
                record(|| event);
            }
        });
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), every.len());
        let names = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(value["schema_version"], 1);
                assert_eq!(value["sequence"], index as u64);
                assert!(value["elapsed_nanos"].is_u64(), "{line}");
                value["event"].as_str().unwrap().to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(names, Event::NAMES);
    }

    /// Nothing is written when nobody asked for a trace.
    #[test]
    fn a_run_that_asked_for_no_trace_records_nothing() {
        let sink = Shared::default();
        let (_, before) = super::testing::held(|| ());
        assert!(before.is_empty(), "{before}");
        // Recording ends with the run, and an event after it is dropped rather
        // than held for whoever records next.
        start_shared(sink.clone());
        stop();
        record(|| Event::ClockRead);
        assert!(sink.text().is_empty(), "{}", sink.text());
    }

    /// No event carries a value that could be a secret.
    ///
    /// The property is the vocabulary's, not each call site's: an environment
    /// variable contributes its name and whether it was set, a file
    /// contributes its path and digest, and neither contributes contents. A
    /// variant that carried a value would fail here rather than in review.
    #[test]
    fn no_event_carries_a_value_read_from_outside() {
        let text = recorded(|| {
            record(|| Event::EnvironmentRead {
                name: "GITHUB_TOKEN".into(),
                present: true,
            });
            record(|| Event::ClockRead);
            record(|| Event::Digest {
                bytes: 64,
                digest: "sha256:cc".into(),
            });
        });
        assert!(text.contains("GITHUB_TOKEN"), "{text}");
        for value in ["ghp_", "secret", "hunter2"] {
            assert!(!text.contains(value), "{text}");
        }
        // The names of the fields a value could arrive in, so a variant that
        // added one is caught here.
        for field in ["value", "contents", "stdout", "stderr", "stdin", "body"] {
            assert!(!text.contains(&format!("\"{field}\"")), "{text}");
        }
    }
}
