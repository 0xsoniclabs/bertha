use std::cmp::{self, Ordering};

use crate::BlockRange;

pub trait RangesExt {
    /// Adds a range to the ranges.
    ///
    /// Postconditions:
    /// - ranges are non-empty (start <= end)
    /// - ranges are non-overlapping
    /// - ranges are sorted
    fn add_range(&mut self, new: BlockRange);

    /// Subtracts a range from the ranges.
    ///
    /// Postconditions:
    /// - ranges are non-empty (start <= end)
    /// - ranges are non-overlapping
    /// - ranges are sorted
    fn subtract_range(&mut self, del: &BlockRange);

    /// Sorts the ranges, removes all empty ranges and merges overlapping or adjacent ranges.
    ///
    /// Postconditions:
    /// - ranges are non-empty (start <= end)
    /// - ranges are non-overlapping
    /// - ranges are sorted
    fn canonicalize(&mut self);
}

impl RangesExt for Vec<BlockRange> {
    fn add_range(&mut self, new: BlockRange) {
        self.push(new);
        self.canonicalize();
    }

    fn subtract_range(&mut self, del: &BlockRange) {
        self.canonicalize();
        let del_start = *del.start();
        let del_end = *del.end();

        let mut i = 0;
        while let Some(range) = self.get(i) {
            let start = *range.start();
            let end = *range.end();
            match (
                del_end.cmp(range.start()),
                del_start.cmp(range.end()),
                del_start.cmp(range.start()),
                del_end.cmp(range.end()),
            ) {
                // The deletion range does not overlap with the current range
                (Ordering::Less, _, _, _) | (_, Ordering::Greater, _, _) => {
                    i += 1;
                }
                // The deletion range contains the current range: remove and continue
                (_, _, Ordering::Less | Ordering::Equal, Ordering::Greater | Ordering::Equal) => {
                    self.remove(i);
                }
                // Overlap at end of existing range: trim right and continue to check if it overlaps
                // with one of the following ranges
                (_, _, Ordering::Greater, Ordering::Greater | Ordering::Equal) => {
                    self[i] = start..=del_start - 1;
                    i += 1;
                }
                // Overlap at start of existing range: trim left
                (_, _, Ordering::Less | Ordering::Equal, Ordering::Less) => {
                    self[i] = del_end + 1..=end;
                    i += 1;
                }
                // The deletion range is contained in the current range: split into two ranges
                (_, _, Ordering::Greater, Ordering::Less) => {
                    self[i] = start..=del_start - 1;
                    self.insert(i + 1, (del_end + 1)..=end);
                    i += 2;
                }
            }
        }
    }

    fn canonicalize(&mut self) {
        self.sort_by_key(|r| (*r.start(), *r.end()));
        let mut i = 0;
        while let Some(range) = self.get(i) {
            let start = *range.start();
            let end = *range.end();
            if end < start {
                self.remove(i);
                continue;
            }
            if i + 1 < self.len() && end + 1 >= *self[i + 1].start() {
                // Overlapping or adjacent ranges, merge them
                self[i] = start..=cmp::max(end, *self[i + 1].end());
                self.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }
}

/// Subtracts multiple ranges from a range.
///
/// Postconditions:
/// - ranges are non-empty (start <= end)
/// - ranges are non-overlapping
/// - ranges are sorted
pub fn subtract_ranges(minuend: BlockRange, subtrahend: &[BlockRange]) -> Vec<BlockRange> {
    let mut segments = vec![minuend];
    for range in subtrahend {
        segments.subtract_range(range);
    }
    segments
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};

    use super::*;

    #[test]
    fn add_range_to_ranges_adds_new_part_of_range_and_maintains_invariants() {
        let mut rng = rand::rng();
        // test cases in the form of (existing ranges, new range, expected ranges)
        let cases = [
            // add range to empty ranges
            (vec![], 0..=1, vec![0..=1]),
            //
            // add range before all existing ranges (non-adjacent)
            (vec![3..=4], 0..=1, vec![0..=1, 3..=4]),
            // add range before all existing ranges (adjacent)
            (vec![2..=3], 0..=1, vec![0..=3]),
            //
            // add range between existing ranges (non-adjacent)
            (vec![0..=1, 6..=7], 3..=4, vec![0..=1, 3..=4, 6..=7]),
            // add range between existing ranges (non-adjacent left, adjacent right)
            (vec![0..=1, 5..=6], 3..=4, vec![0..=1, 3..=6]),
            // add range between existing ranges (adjacent left, non-adjacent right)
            (vec![0..=1, 5..=6], 2..=3, vec![0..=3, 5..=6]),
            //
            // add range after all existing ranges (non-adjacent)
            (vec![0..=1], 3..=4, vec![0..=1, 3..=4]),
            // add range after all existing ranges (adjacent)
            (vec![0..=1], 2..=3, vec![0..=3]),
            //
            // add range contained in existing range: non-adjacent
            (vec![0..=3], 1..=2, vec![0..=3]),
            // add range contained in existing range: left adjacent
            (vec![0..=2], 0..=1, vec![0..=2]),
            // add range contained in existing range: right adjacent
            (vec![0..=2], 1..=2, vec![0..=2]),
            // add range contained in existing range: adjacent
            (vec![0..=1], 0..=1, vec![0..=1]),
            //
            // add range filling gap between existing ranges
            (vec![0..=1, 3..=4], 2..=2, vec![0..=4]),
            //
            // add range partially spanning multiple ranges
            (vec![1..=2, 4..=5], 2..=4, vec![1..=5]),
            //
            // add range fully spanning multiple ranges: span ranges exact
            (vec![0..=1, 3..=4], 0..=4, vec![0..=4]),
            // add range fully spanning multiple ranges: span even larger than ranges
            (vec![1..=2, 4..=5], 0..=6, vec![0..=6]),
        ];
        for (mut ranges, new_range, expected) in cases {
            ranges.add_range(new_range.clone());
            assert_eq!(ranges, expected);
            // valid
            assert!(ranges.iter().all(|range| range.start() <= range.end()));
            // non overlapping
            assert!(ranges.windows(2).all(|w| w[0].end() < w[1].start()));
            // sorted
            assert!(ranges.is_sorted_by_key(|r| (r.start(), r.end())));

            ranges.shuffle(&mut rng);
            if !ranges.is_empty() {
                let range = ranges[0].clone();
                if range.start() < range.end() {
                    // add overlapping ranges
                    ranges.push(*range.start()..=*range.end() - 1);
                    ranges.push(*range.start() + 1..=*range.end());
                } else {
                    // duplicate range
                    ranges.push(range);
                }
            }
            ranges.add_range(new_range);
            assert_eq!(ranges, expected);
            // valid
            assert!(ranges.iter().all(|range| range.start() <= range.end()));
            // non overlapping
            assert!(ranges.windows(2).all(|w| w[0].end() < w[1].start()));
            // sorted
            assert!(ranges.is_sorted_by_key(|r| (r.start(), r.end())));
        }
    }

    #[test]
    fn delete_range_from_ranges_removes_range_and_maintains_invariants() {
        let mut rng = rand::rng();
        // test cases in the form of (existing ranges, range to delete, expected ranges)
        let cases = [
            // remove non-existing range from empty ranges
            (vec![], 0..=1, vec![]),
            // remove non-existing range before all existing ranges
            (vec![2..=3], 0..=1, vec![2..=3]),
            // remove non-existing range after all existing ranges
            (vec![0..=1], 2..=3, vec![0..=1]),
            //
            // remove start of existing range
            (vec![0..=3], 0..=1, vec![2..=3]),
            // remove end of existing range
            (vec![0..=3], 2..=3, vec![0..=1]),
            // remove middle of existing range
            (vec![0..=3], 1..=2, vec![0..=0, 3..=3]),
            // remove full existing range
            (vec![0..=3], 0..=3, vec![]),
            // remove range that is larger than existing range
            (vec![1..=2], 0..=3, vec![]),
            //
            // remove range that spans parts of multiple existing ranges
            (vec![0..=1, 3..=4], 1..=3, vec![0..=0, 4..=4]),
            // remove range that spans multiple existing ranges
            (vec![0..=1, 3..=4], 0..=4, vec![]),
            // remove range that is larger than multiple existing ranges
            (vec![1..=1, 3..=3], 0..=4, vec![]),
        ];
        for (mut ranges, del_range, expected) in cases {
            ranges.subtract_range(&del_range);
            assert_eq!(ranges, expected);
            // valid
            assert!(ranges.iter().all(|range| range.start() <= range.end()));
            // non overlapping
            assert!(ranges.windows(2).all(|w| w[0].end() < w[1].start()));
            // sorted
            assert!(ranges.is_sorted_by_key(|r| (r.start(), r.end())));

            ranges.shuffle(&mut rng);
            if !ranges.is_empty() {
                let range = ranges[0].clone();
                if range.start() < range.end() {
                    // add overlapping ranges
                    ranges.push(*range.start()..=*range.end() - 1);
                    ranges.push(*range.start() + 1..=*range.end());
                } else {
                    // duplicate range
                    ranges.push(range);
                }
            }
            ranges.subtract_range(&del_range);
            assert_eq!(ranges, expected);
            // valid
            assert!(ranges.iter().all(|range| range.start() <= range.end()));
            // non overlapping
            assert!(ranges.windows(2).all(|w| w[0].end() < w[1].start()));
            // sorted
            assert!(ranges.is_sorted_by_key(|r| (r.start(), r.end())));
        }
    }

    #[test]
    fn subtract_ranges_computes_correct_difference() {
        // test cases in the form of (existing range, ranges to delete, expected ranges)
        let cases = vec![
            // No local ranges, should return the whole range
            (0..=30, vec![], vec![0..=30]),
            // Whole range already available locally, should return empty
            (0..=30, vec![0..=30], vec![]),
            // Local ranges do not cover the whole range, should return the missing parts
            (
                0..=30,
                vec![0..=5, 7..=10, 15..=20, 22..=28, 30..=30],
                vec![6..=6, 11..=14, 21..=21, 29..=29],
            ),
            // Missing end of the range
            (0..=30, vec![0..=20], vec![21..=30]),
            // Missing start of the range
            (0..=30, vec![11..=30], vec![0..=10]),
            // Missing both ends of the range (duplicate of above for completeness)
            (0..=30, vec![11..=20], vec![0..=10, 21..=30]),
            // Difference is equal to a single block (not locally available)
            (5..=10, vec![5..=5, 7..=10, 15..=20], vec![6..=6]),
            // Requested range is a single block (locally available)
            (15..=15, vec![5..=5, 7..=10, 15..=20], vec![]),
            // Requested range is a single block (not locally available)
            (15..=15, vec![5..=5, 7..=10, 16..=20], vec![15..=15]),
        ];

        let mut rng = SmallRng::seed_from_u64(123);
        for (range, del, expected) in cases {
            let diff = subtract_ranges(range.clone(), &del);
            assert_eq!(
                diff, expected,
                "Failed for requested_range: {range:?}, local_ranges: {del:?}"
            );
            // Randomize the order of local ranges to ensure that we don't rely on them being
            // sorted
            let mut randomized_local_ranges = del.clone();
            randomized_local_ranges.shuffle(&mut rng);
            let diff = subtract_ranges(range.clone(), &randomized_local_ranges);
            assert_eq!(
                diff, expected,
                "Failed for requested_range: {range:?}, shuffled local_ranges: {randomized_local_ranges:?}"
            );
        }
    }
}
