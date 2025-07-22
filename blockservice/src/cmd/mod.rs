use std::fmt::Write;

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
