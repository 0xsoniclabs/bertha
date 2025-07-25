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
                del_end.cmp(&start),
                del_start.cmp(&end),
                del_start.cmp(&start),
                del_end.cmp(&end),
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
            if range.is_empty() {
                self.remove(i);
                continue;
            }
            let start = *range.start();
            let end = *range.end();
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

/// Intersects a target range with a list of candidate ranges.
/// It returns the union of all intersections of the target range with the candidate ranges.
///
///  Postconditions:
/// - ranges are non-empty (start <= end)
/// - ranges are non-overlapping
/// - ranges are sorted
#[cfg(test)]
pub fn intersect_ranges(target: BlockRange, candidates: &[BlockRange]) -> Vec<BlockRange> {
    let mut segments = Vec::new();
    for candidate in candidates {
        if candidate.start() <= target.end() && candidate.end() >= target.start() {
            let start = cmp::max(*candidate.start(), *target.start());
            let end = cmp::min(*candidate.end(), *target.end());
            segments.push(start..=end);
        }
    }
    segments.canonicalize();
    segments
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
    fn add_range_adds_new_part_of_range_and_maintains_invariants() {
        let mut rng = SmallRng::seed_from_u64(123);
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
            assert_eq_expected_and_postconditions(&ranges, &expected);

            shuffle_and_make_overlapping(&mut ranges, &mut rng);
            ranges.add_range(new_range);
            assert_eq_expected_and_postconditions(&ranges, &expected);
        }
    }

    #[test]
    fn subtract_range_removes_range_and_maintains_invariants() {
        let mut rng = SmallRng::seed_from_u64(123);
        // test cases in the form of (existing ranges, range to subtract, expected ranges)
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
            assert_eq_expected_and_postconditions(&ranges, &expected);

            shuffle_and_make_overlapping(&mut ranges, &mut rng);
            ranges.subtract_range(&del_range);
            assert_eq_expected_and_postconditions(&ranges, &expected);
        }
    }

    #[test]
    fn subtract_ranges_computes_correct_difference() {
        // test cases in the form of (existing range, ranges to subtract, expected ranges)
        let cases = vec![
            // Remove nothing
            (0..=1, vec![], vec![0..=1]),
            // Remove non-existing range
            (0..=1, vec![2..=3], vec![0..=1]),
            // Remove range that covers the whole existing range
            (0..=1, vec![0..=1], vec![]),
            // Remove start of existing range
            (0..=3, vec![0..=2], vec![3..=3]),
            // Remove end of existing range
            (0..=3, vec![1..=3], vec![0..=0]),
            // Remove middle of existing range
            (0..=3, vec![1..=2], vec![0..=0, 3..=3]),
            // Remove everything but one block
            (0..=3, vec![0..=1, 3..=3], vec![2..=2]),
            // Remove multiple parts of the existing range
            (0..=4, vec![0..=0, 2..=2, 4..=4], vec![1..=1, 3..=3]),
        ];

        let mut rng = SmallRng::seed_from_u64(123);
        for (range, mut del, expected) in cases {
            let diff = subtract_ranges(range.clone(), &del);
            assert_eq_expected_and_postconditions(&diff, &expected);

            shuffle_and_make_overlapping(&mut del, &mut rng);
            let diff = subtract_ranges(range.clone(), &del);
            assert_eq_expected_and_postconditions(&diff, &expected);
        }
    }

    #[test]
    fn canonicalize_sorts_and_merges_and_removes_empty_ranges() {
        #[allow(clippy::reversed_empty_ranges)]
        let cases = [
            // no ranges
            (vec![], vec![]),
            // unsorted ranges
            (vec![6..=7, 1..=2, 4..=4], vec![1..=2, 4..=4, 6..=7]),
            // overlapping ranges
            (vec![1..=2, 2..=3, 3..=4], vec![1..=4]),
            // adjacent ranges
            (vec![1..=2, 3..=4, 5..=6], vec![1..=6]),
            // empty ranges
            (vec![1..=0], vec![]),
            // unsorted and overlapping and adjacent and empty ranges
            (vec![3..=4, 1..=2, 3..=3, 6..=7, 1..=0], vec![1..=4, 6..=7]),
        ];
        for (mut ranges, expected) in cases {
            ranges.canonicalize();
            assert_eq_expected_and_postconditions(&ranges, &expected);
        }
    }

    #[test]
    fn intersect_ranges_returns_correct_intersections() {
        // test cases in the form of (target range, candidate ranges, expected intersection)
        let cases = [
            // intersection with empty candidates
            (0..=1, vec![], vec![]),
            // intersection with non-overlapping candidates
            (0..=1, vec![2..=3], vec![]),
            // target range is equal to candidate range
            (0..=1, vec![0..=1], vec![0..=1]),
            // target range contains all the candidates
            (0..=3, vec![1..=2, 2..=3], vec![1..=3]),
            // target range is contained in a candidate
            (1..=2, vec![0..=3], vec![1..=2]),
            // target range overlaps with non-adjacent candidates
            (1..=5, vec![0..=2, 4..=6], vec![1..=2, 4..=5]),
        ];

        for (target, candidates, expected) in cases {
            let result = intersect_ranges(target.clone(), &candidates);
            assert_eq_expected_and_postconditions(&result, &expected);

            let mut shuffled_candidates = candidates.clone();
            let mut rng = SmallRng::seed_from_u64(123);
            shuffle_and_make_overlapping(&mut shuffled_candidates, &mut rng);
            let result = intersect_ranges(target, &shuffled_candidates);
            assert_eq_expected_and_postconditions(&result, &expected);
        }
    }

    #[track_caller]
    fn assert_eq_expected_and_postconditions(ranges: &[BlockRange], expected: &[BlockRange]) {
        assert_eq!(ranges, expected, "ranges do not match expected ranges");
        assert!(
            ranges.iter().all(|range| range.start() <= range.end()),
            "ranges are not valid"
        );
        assert!(
            ranges.windows(2).all(|w| w[0].end() < w[1].start()),
            "ranges are not non-overlapping"
        );
        assert!(
            ranges.is_sorted_by_key(|r| (r.start(), r.end())),
            "ranges are not sorted"
        );
    }

    fn shuffle_and_make_overlapping(ranges: &mut Vec<BlockRange>, rng: &mut SmallRng) {
        ranges.shuffle(rng);
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
    }
}
