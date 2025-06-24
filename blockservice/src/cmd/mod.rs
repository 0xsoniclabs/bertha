#[cfg(test)]
use std::{
    env,
    path::{Path, PathBuf},
};

pub use import::import;
pub use init::init;
pub use purge::purge;
pub use verify::verify;

mod import;
mod init;
mod purge;
mod verify;

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
