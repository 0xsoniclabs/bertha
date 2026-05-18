// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

use std::{io::Read, num::NonZeroUsize};

use crate::error::Error;

/// A wrapper around a [Read] source that provides access to its internal buffer as a slice.
/// It makes sure that before each read operation, the internal buffer is filled at least with
/// [SliceReader::min_slice_size] bytes. If this is not the case, the reader is used to fill the
/// buffer with more data. The only exception is when the reader is empty, in which case the buffer
/// is allowed to hold less elements.
pub struct SliceReader<R: Read> {
    reader: R,
    reader_empty: bool,
    buffer_size: usize,
    min_slice_size: usize,
    buffer: Vec<u8>,
    buffer_idx: usize,
}

impl<R: Read> SliceReader<R> {
    /// Creates a new [SliceReader] with the given source, buffer size and minimum slice size.
    /// The buffer size must be greater than or equal to the minimum slice size.
    /// If this is not the case, the buffer size is set to twice the minimum slice size.
    pub fn new(source: R, mut buffer_size: usize, min_slice_size: NonZeroUsize) -> Self {
        let min_slice_size = min_slice_size.get();
        if buffer_size < min_slice_size {
            buffer_size = min_slice_size * 2;
        }
        Self {
            reader: source,
            reader_empty: false,
            buffer_size,
            min_slice_size,
            buffer: Vec::with_capacity(buffer_size),
            buffer_idx: 0,
        }
    }

    /// Processes the internal buffer with a function `f` that takes a mutable reference to a slice
    /// of the buffer. The function `f` is expected to shrink the slice by the amount of data
    /// that it read. If the buffer is empty, `Ok(None)` is returned, otherwise `Ok(Some(value))`
    /// where `value` is the result of the function `f` if processing succeeded or `Err(error)`
    /// if it failed.
    pub fn process_with<F, O, E>(&mut self, mut f: F) -> Result<Option<O>, Error>
    where
        F: FnMut(&mut &[u8]) -> Result<O, E>,
        E: Into<Error>,
    {
        self.fill_buffer()?;
        let mut slice = &self.buffer[self.buffer_idx..];
        if slice.is_empty() {
            return Ok(None);
        }
        let slice_len = slice.len();
        let res = f(&mut slice);
        let consumed = slice_len - slice.len();
        self.buffer_idx += consumed;
        res.map(Some).map_err(Into::into)
    }

    /// Fills the internal buffer with bytes from the reader if the unconsumed part of the buffer
    /// (after index [SliceReader::buffer_idx] contains less that [SliceReader::min_slice_size],
    /// unless the reader is empty.
    fn fill_buffer(&mut self) -> Result<(), Error> {
        if self.buffer_idx + self.min_slice_size >= self.buffer.len() && !self.reader_empty {
            let remaining = self.buffer.len() - self.buffer_idx;
            self.buffer.copy_within(self.buffer_idx.., 0);
            self.buffer.truncate(remaining);
            self.buffer_idx = 0;
            self.reader
                .by_ref()
                .take((self.buffer_size - remaining) as u64)
                .read_to_end(&mut self.buffer)?;
            if self.buffer.len() == remaining {
                self.reader_empty = true;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Read, num::NonZeroUsize};

    use super::SliceReader;
    use crate::{Error, GFileError};

    fn consume(slice: &mut &[u8], size: usize) -> Result<(), Error> {
        assert!(
            slice.len() >= size,
            "the slice is smaller than the size to consume"
        );
        *slice = &slice[size..];
        Ok(())
    }

    #[rstest::rstest]
    #[case::buffer_size_greater_than_min_slice_size(2, 1, 2, 1)]
    #[case::buffer_size_equal_to_min_slice_size(1, 1, 1, 1)]
    #[case::buffer_size_less_than_min_slice_size(0, 1, 2, 1)]
    fn new_checks_buffer_size_is_greater_or_equal_to_min_slice_size(
        #[case] buffer_size: usize,
        #[case] min_slice_size: usize,
        #[case] expected_buffer_size: usize,
        #[case] expected_min_slice_size: usize,
    ) {
        let reader = SliceReader::new(
            std::io::empty(),
            buffer_size,
            NonZeroUsize::new(min_slice_size).unwrap(),
        );
        assert_eq!(reader.buffer_size, expected_buffer_size);
        assert_eq!(reader.min_slice_size, expected_min_slice_size);
    }

    #[test]
    fn buffer_has_always_slice_size_unless_reader_is_empty() {
        const BUF_SIZE: usize = 10;
        const MIN_SLICE_SIZE: usize = 6;
        const DATA_SIZE: usize = 28;

        let data = [0u8; DATA_SIZE];
        let mut reader = SliceReader::new(
            data.as_slice(),
            BUF_SIZE,
            NonZeroUsize::new(MIN_SLICE_SIZE).unwrap(),
        );

        for _ in 0..DATA_SIZE / MIN_SLICE_SIZE {
            reader
                .process_with(|slice| {
                    assert!(
                        slice.len() >= MIN_SLICE_SIZE,
                        "buffer length is smaller than SLICE_SIZE"
                    );
                    consume(slice, MIN_SLICE_SIZE)
                })
                .unwrap()
                .unwrap();
        }
        assert!(
            reader
                .process_with(|slice| consume(slice, 3))
                .unwrap()
                .is_some()
        );
        assert!(
            reader
                .process_with(|slice| consume(slice, 1))
                .unwrap()
                .is_some()
        );
        assert!(
            reader
                .process_with(|slice| consume(slice, 1))
                .unwrap()
                .is_none()
        );
        // Now the underlying reader and the unconsumed part of the buffer are empty
        assert!(
            reader
                .process_with(|slice| {
                    assert!(slice.is_empty());
                    Result::<(), Error>::Ok(())
                })
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn process_with_propagates_errors_from_mapping_function() {
        const BUF_SIZE: usize = 10;
        const MIN_SLICE_SIZE: usize = 6;
        const DATA_SIZE: usize = 28;

        let data = [0u8; DATA_SIZE];
        let mut reader = SliceReader::new(
            data.as_slice(),
            BUF_SIZE,
            NonZeroUsize::new(MIN_SLICE_SIZE).unwrap(),
        );

        assert!(matches!(
            reader.process_with(|_| { Err::<(), _>(Error::GFile(GFileError::BlocksUnitMissing)) }),
            Err(Error::GFile(GFileError::BlocksUnitMissing))
        ));
    }

    #[test]
    fn process_with_propagates_errors_from_reader() {
        const BUF_SIZE: usize = 10;
        const MIN_SLICE_SIZE: usize = 6;

        struct ErrorReader;
        impl Read for ErrorReader {
            fn read(&mut self, _buf: &mut [u8]) -> Result<usize, std::io::Error> {
                Err(std::io::Error::other("simulated read error"))
            }
        }

        let mut reader = SliceReader::new(
            ErrorReader,
            BUF_SIZE,
            NonZeroUsize::new(MIN_SLICE_SIZE).unwrap(),
        );

        assert!(
            reader
                .process_with(|_| Result::<(), Error>::Ok(()))
                .unwrap_err()
                .to_string()
                .contains("simulated read error")
        );
    }
}
