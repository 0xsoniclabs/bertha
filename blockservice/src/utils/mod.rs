use std::io::Cursor;

pub mod ranges;
#[cfg(test)]
pub mod test_dir;

/// A trait for getting a buffered reader from different input types.
/// This trait is useful when the type from which the [`BufRead`] is produced implements [`Send`]
/// and [`Sync`], but the [`std::io::BufRead`] type itself does not, because this way we can pass
/// the original type around, and produce the buffered reader when needed.
pub trait InputReader {
    fn get_reader(&self) -> impl std::io::BufRead;
}

/// A implementation of [`InputReader`] for [`Cursor<T>`], which allows reading from in-memory data.
impl<T> InputReader for Cursor<T>
where
    T: AsRef<[u8]> + Clone,
{
    /// Returns the cursor itself as a buffered reader.
    fn get_reader(&self) -> impl std::io::BufRead {
        self.clone()
    }
}
/// A implementation of [`InputReader`] for [`std::io::Stdin`], which locks standard input and
/// allows reading from it.
impl InputReader for std::io::Stdin {
    /// Lock the standard input and return it as a buffered reader.
    fn get_reader(&self) -> impl std::io::BufRead {
        self.lock()
    }
}
