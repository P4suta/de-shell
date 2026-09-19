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
