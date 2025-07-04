use std::fmt::Write;
#[cfg(test)]
use std::{
    env,
    path::{Path, PathBuf},
};

pub use fetch::fetch;
pub use import::import;
use indicatif::{ProgressBar, style::TemplateError};
pub use init::init;
pub use list::list;
pub use purge::purge;
pub use start::start;
pub use verify::verify;
pub use view::view;

mod fetch;
mod import;
mod init;
mod list;
mod purge;
mod start;
mod verify;
mod view;

#[cfg(test)]
static WORKING_DIR_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A guard type to temporarily change the working directory while avoiding
/// race-conditions across concurrently executed tests.
#[cfg(test)]
struct ChangeWorkingDir<'a> {
    _guard: std::sync::MutexGuard<'a, ()>,
    prev: PathBuf,
}

#[cfg(test)]
impl ChangeWorkingDir<'_> {
    fn new(path: impl AsRef<Path>) -> Self {
        let guard = WORKING_DIR_MUTEX.lock().unwrap();
        let prev = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();
        ChangeWorkingDir {
            _guard: guard,
            prev,
        }
    }
}

#[cfg(test)]
impl Drop for ChangeWorkingDir<'_> {
    fn drop(&mut self) {
        env::set_current_dir(&self.prev).unwrap();
    }
}

/// Creates a new progress bar with a custom style and an ETA display.
pub fn make_progress_bar(total: u64) -> Result<ProgressBar, TemplateError> {
    let bar = ProgressBar::new(total);
    bar.set_style(
        indicatif::ProgressStyle::with_template(
            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} (ETA {eta})",
        )?
        .with_key(
            "eta",
            |state: &indicatif::ProgressState, w: &mut dyn Write| {
                // Since there is no way of propagating errors from this closure,
                // we just ignore the result (worst case the ETA will not be shown).
                let _ = write!(w, "{:.1}s", state.eta().as_secs_f64());
            },
        )
        .progress_chars("#>-"),
    );
    Ok(bar)
}
