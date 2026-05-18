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

    #[rstest::rstest]
    #[case::add_to_empty(vec![], 0..=1, vec![0..=1])]
    #[case::add_before_non_adjacent(vec![3..=4], 0..=1, vec![0..=1, 3..=4])]
    #[case::add_before_adjacent(vec![2..=3], 0..=1, vec![0..=3])]
    #[case::add_between_non_adjacent(vec![0..=1, 6..=7], 3..=4, vec![0..=1, 3..=4, 6..=7])]
    #[case::add_between_non_adjacent_left_adjacent_right(vec![0..=1, 5..=6], 3..=4, vec![0..=1, 3..=6])]
    #[case::add_between_adjacent_left_non_adjacent_right(vec![0..=1, 5..=6], 2..=3, vec![0..=3, 5..=6])]
    #[case::add_after_non_adjacent(vec![0..=1], 3..=4, vec![0..=1, 3..=4])]
    #[case::add_after_adjacent(vec![0..=1], 2..=3, vec![0..=3])]
    #[case::add_contained_non_adjacent(vec![0..=3], 1..=2, vec![0..=3])]
    #[case::add_contained_left_adjacent(vec![0..=2], 0..=1, vec![0..=2])]
    #[case::add_contained_right_adjacent(vec![0..=2], 1..=2, vec![0..=2])]
    #[case::add_contained_equal(vec![0..=1], 0..=1, vec![0..=1])]
    #[case::add_fills_gap(vec![0..=1, 3..=4], 2..=2, vec![0..=4])]
    #[case::add_partially_spans_multiple(vec![1..=2, 4..=5], 2..=4, vec![1..=5])]
    #[case::add_fully_spans_multiple_exact(vec![0..=1, 3..=4], 0..=4, vec![0..=4])]
    #[case::add_fully_spans_multiple_larger(vec![1..=2, 4..=5], 0..=6, vec![0..=6])]
    fn add_range_adds_new_part_of_range_and_maintains_invariants(
        #[case] mut ranges: Vec<BlockRange>,
        #[case] new_range: BlockRange,
        #[case] expected: Vec<BlockRange>,
    ) {
        let mut rng = SmallRng::seed_from_u64(123);
        ranges.add_range(new_range.clone());
        assert_eq_expected_and_postconditions(&ranges, &expected);

        shuffle_and_make_overlapping(&mut ranges, &mut rng);
        ranges.add_range(new_range);
        assert_eq_expected_and_postconditions(&ranges, &expected);
    }

    #[rstest::rstest]
    #[case::remove_nonexisting_from_empty(vec![], 0..=1, vec![])]
    #[case::remove_nonexisting_before(vec![2..=3], 0..=1, vec![2..=3])]
    #[case::remove_nonexisting_after(vec![0..=1], 2..=3, vec![0..=1])]
    #[case::remove_start_of_range(vec![0..=3], 0..=1, vec![2..=3])]
    #[case::remove_end_of_range(vec![0..=3], 2..=3, vec![0..=1])]
    #[case::remove_middle_of_range(vec![0..=3], 1..=2, vec![0..=0, 3..=3])]
    #[case::remove_full_range(vec![0..=3], 0..=3, vec![])]
    #[case::remove_larger_than_range(vec![1..=2], 0..=3, vec![])]
    #[case::remove_spans_parts_of_multiple(vec![0..=1, 3..=4], 1..=3, vec![0..=0, 4..=4])]
    #[case::remove_spans_multiple(vec![0..=1, 3..=4], 0..=4, vec![])]
    #[case::remove_larger_than_multiple(vec![1..=1, 3..=3], 0..=4, vec![])]
    fn subtract_range_removes_range_and_maintains_invariants(
        #[case] mut ranges: Vec<BlockRange>,
        #[case] del_range: BlockRange,
        #[case] expected: Vec<BlockRange>,
    ) {
        let mut rng = SmallRng::seed_from_u64(123);
        ranges.subtract_range(&del_range);
        assert_eq_expected_and_postconditions(&ranges, &expected);

        shuffle_and_make_overlapping(&mut ranges, &mut rng);
        ranges.subtract_range(&del_range);
        assert_eq_expected_and_postconditions(&ranges, &expected);
    }

    #[rstest::rstest]
    #[case::remove_nothing(0..=1, vec![], vec![0..=1])]
    #[case::remove_nonexisting(0..=1, vec![2..=3], vec![0..=1])]
    #[case::remove_covers_all(0..=1, vec![0..=1], vec![])]
    #[case::remove_start(0..=3, vec![0..=2], vec![3..=3])]
    #[case::remove_end(0..=3, vec![1..=3], vec![0..=0])]
    #[case::remove_middle(0..=3, vec![1..=2], vec![0..=0, 3..=3])]
    #[case::remove_all_but_one(0..=3, vec![0..=1, 3..=3], vec![2..=2])]
    #[case::remove_multiple_parts(0..=4, vec![0..=0, 2..=2, 4..=4], vec![1..=1, 3..=3])]
    fn subtract_ranges_computes_correct_difference(
        #[case] range: BlockRange,
        #[case] mut del: Vec<BlockRange>,
        #[case] expected: Vec<BlockRange>,
    ) {
        let mut rng = SmallRng::seed_from_u64(123);
        let diff = subtract_ranges(range.clone(), &del);
        assert_eq_expected_and_postconditions(&diff, &expected);

        shuffle_and_make_overlapping(&mut del, &mut rng);
        let diff = subtract_ranges(range.clone(), &del);
        assert_eq_expected_and_postconditions(&diff, &expected);
    }

    #[rstest::rstest]
    #[case::no_ranges(vec![], vec![])]
    #[case::unsorted(vec![6..=7, 1..=2, 4..=4], vec![1..=2, 4..=4, 6..=7])]
    #[case::overlapping(vec![1..=2, 2..=3, 3..=4], vec![1..=4])]
    #[case::adjacent(vec![1..=2, 3..=4, 5..=6], vec![1..=6])]
    #[allow(clippy::reversed_empty_ranges)]
    #[case::empty_ranges(vec![1..=0], vec![])]
    #[allow(clippy::reversed_empty_ranges)]
    #[case::mixed(vec![3..=4, 1..=2, 3..=3, 6..=7, 1..=0], vec![1..=4, 6..=7])]
    fn canonicalize_sorts_and_merges_and_removes_empty_ranges(
        #[case] mut ranges: Vec<BlockRange>,
        #[case] expected: Vec<BlockRange>,
    ) {
        ranges.canonicalize();
        assert_eq_expected_and_postconditions(&ranges, &expected);
    }

    #[rstest::rstest]
    #[case::empty_candidates(0..=1, vec![], vec![])]
    #[case::non_overlapping(0..=1, vec![2..=3], vec![])]
    #[case::equal(0..=1, vec![0..=1], vec![0..=1])]
    #[case::contains_all(0..=3, vec![1..=2, 2..=3], vec![1..=3])]
    #[case::contained_in_candidate(1..=2, vec![0..=3], vec![1..=2])]
    #[case::overlaps_non_adjacent(1..=5, vec![0..=2, 4..=6], vec![1..=2, 4..=5])]
    fn intersect_ranges_returns_correct_intersections(
        #[case] target: BlockRange,
        #[case] candidates: Vec<BlockRange>,
        #[case] expected: Vec<BlockRange>,
    ) {
        let mut rng = SmallRng::seed_from_u64(123);
        let result = intersect_ranges(target.clone(), &candidates);
        assert_eq_expected_and_postconditions(&result, &expected);

        let mut shuffled_candidates = candidates.clone();
        shuffle_and_make_overlapping(&mut shuffled_candidates, &mut rng);
        let result = intersect_ranges(target, &shuffled_candidates);
        assert_eq_expected_and_postconditions(&result, &expected);
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
