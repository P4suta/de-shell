//! Everything de-shell reads that is neither its arguments nor a file.
//!
//! de-shell claims deterministic output and canonical digest bytes. A tool that
//! makes that claim and then reads the clock or the ambient environment from
//! wherever it happens to need one has a claim nobody can check: the inputs are
//! not written down, so a test cannot supply them and a reader cannot enumerate
//! them.
//!
//! `clippy.toml` bans the raw APIs everywhere else, and this module implements
//! them exactly once. That makes the set of ambient inputs the set of functions
//! below — a list a reader can finish — and it gives the tests one place to
//! stand in for all of them.
//!
//! It is the same shape as [`crate::patch`], for the same reason: an intent
//! named in one place is checkable, and the same intent spelled out at fourteen
//! call sites is not.

use std::ffi::OsString;
use std::process::{Command, ExitStatus};
use std::time::{Duration, SystemTime};

/// What a test supplies in place of the ambient environment.
#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub(crate) struct Fixed {
    /// The variables that are set. A name that is absent reads as unset, which
    /// is the condition most of these call sites branch on.
    pub variables: std::collections::BTreeMap<String, OsString>,
    /// What [`wall_clock`] answers.
    pub wall_clock: Option<SystemTime>,
    /// What every [`Stopwatch`] answers, however long it has really run.
    pub elapsed: Option<Duration>,
}

#[cfg(test)]
thread_local! {
    static FIXED: std::cell::RefCell<Option<Fixed>> = const { std::cell::RefCell::new(None) };
}

/// Run `body` with the ambient environment replaced.
///
/// Thread-local and restored on the way out, including through a panic, so one
/// test cannot leave its environment behind for another running beside it.
#[cfg(test)]
pub(crate) fn with<T>(fixed: Fixed, body: impl FnOnce() -> T) -> T {
    struct Restore(Option<Fixed>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FIXED.with(|cell| *cell.borrow_mut() = self.0.take());
        }
    }
    let previous = FIXED.with(|cell| cell.borrow_mut().replace(fixed));
    let _restore = Restore(previous);
    body()
}

#[cfg(test)]
fn fixed<T>(read: impl FnOnce(&Fixed) -> Option<T>) -> Option<T> {
    FIXED.with(|cell| cell.borrow().as_ref().and_then(read))
}

/// The value of an environment variable, or `None` if it is not set.
///
/// `OsString` rather than `String`: a `PATH` entry that is not UTF-8 is still a
/// `PATH` entry, and a lossy read would name a directory that is not there.
pub(crate) fn variable(name: &str) -> Option<OsString> {
    let value = read_variable(name);
    // The name and whether it was set. Never the value: a variable is where a
    // token lives, and a trace that could hold one would have to be handled
    // like one.
    crate::trace::record(|| crate::trace::Event::EnvironmentRead {
        name: name.to_owned(),
        present: value.is_some(),
    });
    value
}

fn read_variable(name: &str) -> Option<OsString> {
    #[cfg(test)]
    if FIXED.with(|cell| cell.borrow().is_some()) {
        return fixed(|fixed| fixed.variables.get(name).cloned());
    }
    std::env::var_os(name)
}

/// The value of an environment variable that must be text to be usable.
pub(crate) fn text_variable(name: &str) -> Option<String> {
    variable(name).and_then(|value| value.into_string().ok())
}

/// The wall clock, for a timestamp that is recorded rather than compared.
pub(crate) fn wall_clock() -> SystemTime {
    // What it said is not recorded: it is the one reading guaranteed to differ
    // between two runs of the same work, so a trace carrying it could never be
    // compared.
    crate::trace::record(|| crate::trace::Event::ClockRead);
    #[cfg(test)]
    if let Some(fixed) = fixed(|fixed| fixed.wall_clock) {
        return fixed;
    }
    SystemTime::now()
}

/// How long something has been running.
///
/// A monotonic reading, so it is not disturbed by the wall clock moving, and a
/// type rather than a bare `Instant` so a test can answer for it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stopwatch(std::time::Instant);

impl Stopwatch {
    pub(crate) fn start() -> Self {
        Self(std::time::Instant::now())
    }

    pub(crate) fn elapsed(self) -> Duration {
        #[cfg(test)]
        if let Some(fixed) = fixed(|fixed| fixed.elapsed) {
            return fixed;
        }
        self.0.elapsed()
    }
}

/// Start `command` and wait for everything it wrote.
///
/// The three launch verbs are here rather than at each call site for the same
/// reason [`variable`] is: a process is an ambient input, and a tool that
/// claims to know what it ran should be able to say so. `clippy.toml` bans
/// `Command::output`, `Command::status` and `Command::spawn` everywhere else,
/// so a new launch is recorded because it compiles.
pub(crate) fn output(command: &mut Command) -> std::io::Result<std::process::Output> {
    let program = started(command);
    let output = command.output();
    ended(
        &program,
        output.as_ref().ok().and_then(|output| output.status.code()),
    );
    output
}

/// Start `command` and wait for its exit status, leaving its streams alone.
///
/// Nothing in de-shell inherits a child's streams today, so nothing outside a
/// test calls this. It is declared because the ban on `Command::status` needs a
/// destination: a caller who reaches for the raw API is told where to go, and
/// finds it already written and already recording.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared so the `Command::status` ban has a destination; no de-shell launch inherits a child's streams yet"
    )
)]
pub(crate) fn status(command: &mut Command) -> std::io::Result<std::process::ExitStatus> {
    let program = started(command);
    let status = command.status();
    ended(&program, status.as_ref().ok().and_then(ExitStatus::code));
    status
}

/// Start `command` and return without waiting.
///
/// No exit is recorded here: the caller holds the child and is the only thing
/// that can say when it ended. A `process_start` with no `process_exit` is a
/// process this run did not wait for, which is a fact worth being able to read.
pub(crate) fn spawn(command: &mut Command) -> std::io::Result<std::process::Child> {
    let _program = started(command);
    command.spawn()
}

/// Record a launch, and return the program name for the exit that follows.
fn started(command: &Command) -> String {
    let program = command.get_program().to_string_lossy().into_owned();
    crate::trace::record(|| crate::trace::Event::ProcessStart {
        program: program.clone(),
        argv: command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect(),
    });
    program
}

fn ended(program: &str, code: Option<i32>) {
    crate::trace::record(|| crate::trace::Event::ProcessExit {
        program: program.to_owned(),
        code,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ambient environment is answered by the fixture, and the fixture is
    /// gone again afterwards.
    #[test]
    fn a_fixed_environment_answers_every_reader_and_does_not_outlive_its_scope() {
        let real = variable("PATH");
        let started = Stopwatch::start();
        with(
            Fixed {
                variables: [("PATH".to_owned(), OsString::from("/only/this"))]
                    .into_iter()
                    .collect(),
                wall_clock: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)),
                elapsed: Some(Duration::from_secs(42)),
            },
            || {
                assert_eq!(variable("PATH"), Some(OsString::from("/only/this")));
                assert_eq!(text_variable("PATH").as_deref(), Some("/only/this"));
                // A name the fixture does not set reads as unset, whatever the
                // real environment holds.
                assert_eq!(variable("HOME"), None);
                assert_eq!(
                    wall_clock(),
                    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)
                );
                assert_eq!(Stopwatch::start().elapsed(), Duration::from_secs(42));
                assert_eq!(started.elapsed(), Duration::from_secs(42));
            },
        );
        assert_eq!(variable("PATH"), real);
        assert!(started.elapsed() < Duration::from_secs(42));
    }

    /// A launch records what it ran and how it ended.
    ///
    /// The three verbs, and the difference between them that matters to a
    /// reader: a spawned process records a start and no exit, because the
    /// caller holds the child and this is not the thing that can say when it
    /// ended.
    #[cfg(unix)]
    #[test]
    fn every_launch_verb_records_the_program_it_ran_and_how_it_ended() {
        let events = |body: fn()| {
            crate::trace::testing::events(&crate::trace::testing::recorded(body))
                .into_iter()
                .filter(|value| {
                    value["event"]
                        .as_str()
                        .is_some_and(|event| event.starts_with("process_"))
                })
                .map(|value| {
                    format!(
                        "{} {} {} {}",
                        value["event"].as_str().unwrap(),
                        value["program"].as_str().unwrap(),
                        value["argv"],
                        value["code"],
                    )
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(
            events(|| {
                let output =
                    output(std::process::Command::new("/bin/sh").args(["-c", "exit 3"])).unwrap();
                assert_eq!(output.status.code(), Some(3));
            }),
            vec![
                r#"process_start /bin/sh ["-c","exit 3"] null"#,
                "process_exit /bin/sh null 3",
            ]
        );
        assert_eq!(
            events(|| {
                let code = status(std::process::Command::new("/bin/sh").args(["-c", "exit 0"]))
                    .unwrap()
                    .code();
                assert_eq!(code, Some(0));
            }),
            vec![
                r#"process_start /bin/sh ["-c","exit 0"] null"#,
                "process_exit /bin/sh null 0",
            ]
        );
        assert_eq!(
            events(|| {
                let mut child =
                    spawn(std::process::Command::new("/bin/sh").args(["-c", "exit 0"])).unwrap();
                child.wait().unwrap();
            }),
            vec![r#"process_start /bin/sh ["-c","exit 0"] null"#]
        );
    }

    #[cfg(windows)]
    #[test]
    fn status_records_the_exact_windows_program_arguments_and_exit_code() {
        let text = crate::trace::testing::recorded(|| {
            let code =
                status(std::process::Command::new("cmd").args(["/D", "/S", "/C", "exit /b 7"]))
                    .unwrap()
                    .code();
            assert_eq!(code, Some(7));
        });
        let events = crate::trace::testing::events(&text)
            .into_iter()
            .filter(|value| {
                value["event"]
                    .as_str()
                    .is_some_and(|event| event.starts_with("process_"))
            })
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"], "process_start");
        assert_eq!(events[0]["program"], "cmd");
        assert_eq!(
            events[0]["argv"],
            serde_json::json!(["/D", "/S", "/C", "exit /b 7"])
        );
        assert_eq!(events[1]["event"], "process_exit");
        assert_eq!(events[1]["program"], "cmd");
        assert_eq!(events[1]["code"], 7);
    }

    /// A panic inside the scope still restores what was there before.
    #[test]
    fn a_panic_inside_a_fixed_environment_restores_the_previous_one() {
        let real = variable("PATH");
        let panicked = std::panic::catch_unwind(|| {
            with(
                Fixed {
                    variables: [("PATH".to_owned(), OsString::from("/gone"))]
                        .into_iter()
                        .collect(),
                    ..Fixed::default()
                },
                || panic!("inside"),
            )
        });
        assert!(panicked.is_err());
        assert_eq!(variable("PATH"), real);
    }
}
