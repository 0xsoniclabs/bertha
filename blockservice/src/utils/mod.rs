use std::io::Cursor;

pub mod ranges;
#[cfg(test)]
pub mod test_dir;

/// A trait for getting a buffered reader from different input types.
/// This trait is useful for bypassing `Send` and `Sync` requirements when passing `BufRead` types
/// around threads.
pub trait InputReader {
    fn get_reader(self) -> impl std::io::BufRead;
}

/// A implementation of `InputReader` for `Cursor<T>`, which allows reading from in-memory data.
impl<T> InputReader for Cursor<T>
where
    T: AsRef<[u8]> + Clone,
{
    /// Returns the cursor itself as a buffered reader.
    fn get_reader(self) -> impl std::io::BufRead {
        self
    }
}

/// A implementation of `InputReader` for `std::io::Stdin`, which allows reading from standard
/// input.
impl InputReader for std::io::Stdin {
    /// Lock the standard input and return it as a buffered reader.
    fn get_reader(self) -> impl std::io::BufRead {
        self.lock()
    }
}
