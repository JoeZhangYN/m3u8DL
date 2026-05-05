//! Property test for `OrderedSegments::from_indexed`. The newtype's documented invariant
//! is "sorts by SegmentIndex regardless of input order"; the existing ffmpeg_muxer.rs
//! tests only feed already-sorted or empty input, so a regression that drops `sort_by_key`
//! would silently slip through. This generator-based test covers any permutation.

use m3u8dl_server::domain::SegmentIndex;
use m3u8dl_server::ports::muxer::OrderedSegments;
use proptest::prelude::*;
use std::path::PathBuf;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// For any Vec<(SegmentIndex, PathBuf)> input, the output paths are in strict
    /// ascending SegmentIndex order. Length preserved. Identity-via-index preserved
    /// (path with idx N comes out at the position where its idx is the Nth smallest).
    #[test]
    fn from_indexed_sorts_by_segment_index(
        // up to 16 distinct indices in [0, 1000), shuffled
        idx_pool in proptest::collection::hash_set(0u32..1000, 1..=16),
    ) {
        let mut indices: Vec<u32> = idx_pool.into_iter().collect();
        // Shuffle by reversing — proptest's strategy doesn't expose a shuffle directly,
        // but reversing a sorted-by-collection result is enough to verify the sort works
        // for non-sorted input.
        indices.reverse();

        let pairs: Vec<(SegmentIndex, PathBuf)> = indices
            .iter()
            .map(|i| (SegmentIndex(*i), PathBuf::from(format!("seg-{i:06}.ts"))))
            .collect();

        let n = pairs.len();
        let ordered = OrderedSegments::from_indexed(pairs);

        // length preserved
        prop_assert_eq!(ordered.len(), n);

        // each path corresponds to its sorted index
        let mut sorted_indices = indices.clone();
        sorted_indices.sort_unstable();
        let paths = ordered.paths();
        for (i, expected_idx) in sorted_indices.iter().enumerate() {
            let expected_path = format!("seg-{expected_idx:06}.ts");
            let actual_path = paths[i].to_string_lossy().into_owned();
            prop_assert_eq!(actual_path, expected_path);
        }
    }

    /// Empty input → empty output (boundary).
    #[test]
    fn empty_input_yields_empty(
        _seed in 0u32..10,
    ) {
        let ordered = OrderedSegments::from_indexed(vec![]);
        prop_assert!(ordered.is_empty());
        prop_assert_eq!(ordered.len(), 0);
    }

    /// Reverse-sorted input → output still ascending (modal regression: someone
    /// could swap `sort_by_key` for a no-op or `reverse` and the existing happy-path
    /// tests with ≤2 segments wouldn't catch it).
    #[test]
    fn reverse_sorted_input_outputs_ascending(
        n in 2usize..=12,
    ) {
        let pairs: Vec<(SegmentIndex, PathBuf)> = (0..n)
            .rev()
            .map(|i| (SegmentIndex(i as u32), PathBuf::from(format!("seg-{i:06}.ts"))))
            .collect();

        let ordered = OrderedSegments::from_indexed(pairs);
        for (i, path) in ordered.paths().iter().enumerate() {
            let expected = format!("seg-{i:06}.ts");
            let actual = path.to_string_lossy().into_owned();
            prop_assert_eq!(actual, expected);
        }
    }
}
