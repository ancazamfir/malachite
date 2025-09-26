use std::cmp::max;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use malachitebft_core_types::{Context, Height};
use malachitebft_peer::PeerId;

use crate::OutboundRequestId;

/// Manages pending sync requests with optimized range operations.
///
/// Maintains an invariant that `next_uncovered_range` is always up-to-date and represents
/// the next range that should be requested (with the smallest start height).
///
/// Assumptions for optimization:
/// 1. All ranges are disjoint (no overlapping requests)
/// 2. No pending requests end before the initial sync height
#[derive(Debug)]
pub struct PendingRequests<Ctx: Context> {
    /// Map of request ID to (range, peer_id)
    requests: BTreeMap<OutboundRequestId, (RangeInclusive<Ctx::Height>, PeerId)>,

    /// Current sync height (starting point for next range calculation)
    current_height: Ctx::Height,

    /// Maximum batch size for ranges
    max_batch_size: u64,

    /// Pre-computed next uncovered range (always up-to-date)
    next_uncovered_range: RangeInclusive<Ctx::Height>,
}

impl<Ctx: Context> PendingRequests<Ctx> {
    pub fn new(initial_height: Ctx::Height, max_batch_size: u64) -> Self {
        let max_batch_size = max(1, max_batch_size);

        // Compute initial next uncovered range
        let mut end_height = initial_height;
        for _ in 1..max_batch_size {
            end_height = end_height.increment();
        }
        let next_uncovered_range = initial_height..=end_height;

        Self {
            requests: BTreeMap::new(),
            current_height: initial_height,
            max_batch_size,
            next_uncovered_range,
        }
    }

    /// Update the current sync height and recalculate the next uncovered range
    pub fn update_current_height(&mut self, new_height: Ctx::Height) {
        self.current_height = new_height;
        self.update_next_range();
    }

    /// Insert a new pending request
    pub fn insert(
        &mut self,
        request_id: OutboundRequestId,
        range: RangeInclusive<Ctx::Height>,
        peer_id: PeerId,
    ) {
        self.requests.insert(request_id, (range.clone(), peer_id));
        self.update_next_range_after_insert(&range);
    }

    /// Remove a pending request
    pub fn remove(
        &mut self,
        request_id: &OutboundRequestId,
    ) -> Option<(RangeInclusive<Ctx::Height>, PeerId)> {
        let result = self.requests.remove(request_id);
        if let Some((removed_range, _)) = &result {
            self.update_next_range_after_remove(removed_range);
        }
        result
    }

    /// Get a pending request by ID
    pub fn get(
        &self,
        request_id: &OutboundRequestId,
    ) -> Option<&(RangeInclusive<Ctx::Height>, PeerId)> {
        self.requests.get(request_id)
    }

    /// Get the number of pending requests
    pub fn len(&self) -> usize {
        self.requests.len()
    }

    /// Check if there are no pending requests
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// Clear all pending requests
    pub fn clear(&mut self) {
        self.requests.clear();
        self.update_next_range();
    }

    /// Retain only requests that match the given predicate
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&OutboundRequestId, &mut (RangeInclusive<Ctx::Height>, PeerId)) -> bool,
    {
        let old_len = self.requests.len();
        self.requests.retain(f);
        if self.requests.len() != old_len {
            self.update_next_range();
        }
    }

    /// Get all values (range, peer_id pairs)
    pub fn values(&self) -> impl Iterator<Item = &(RangeInclusive<Ctx::Height>, PeerId)> {
        self.requests.values()
    }

    /// Get the next uncovered range that should be requested.
    ///
    /// This is always up-to-date and represents the next range with the smallest start height
    /// that is not covered by any pending request.
    ///
    /// Time complexity: O(1)
    pub fn next_uncovered_range(&self) -> RangeInclusive<Ctx::Height> {
        self.next_uncovered_range.clone()
    }

    /// Update the next uncovered range based on current state.
    ///
    /// This method recalculates the next uncovered range and should be called
    /// whenever requests are added, removed, or the current height changes.
    fn update_next_range(&mut self) {
        self.next_uncovered_range = self.compute_next_uncovered_range();
    }

    /// Smart update after inserting a range - uses knowledge of the specific inserted range
    fn update_next_range_after_insert(&mut self, inserted_range: &RangeInclusive<Ctx::Height>) {
        // Quick check: if inserted range starts after our current next range, no change needed
        if inserted_range.start().as_u64() > self.next_uncovered_range.end().as_u64() {
            return;
        }

        // If inserted range conflicts with our current next range, move next range after it
        if inserted_range.start().as_u64() <= self.next_uncovered_range.end().as_u64()
            && inserted_range.end().as_u64() >= self.next_uncovered_range.start().as_u64()
        {
            // There's overlap - compute new next range starting after the inserted range
            let new_start = inserted_range.end().increment();
            self.next_uncovered_range = self.compute_next_uncovered_range_from(new_start);
            return;
        }

        // Edge case: inserted range might affect our current_height, need full recompute
        if inserted_range.contains(&self.current_height) {
            self.update_next_range();
            return;
        }

        // No conflict, keep current next range
    }

    /// Smart update after removing a range - uses knowledge of the specific removed range  
    fn update_next_range_after_remove(&mut self, removed_range: &RangeInclusive<Ctx::Height>) {
        // Quick check: if removed range ends before our current next range, we might start earlier
        if removed_range.end().as_u64() < self.next_uncovered_range.start().as_u64() {
            // Check if we can now start from the removed range
            let potential_start = max(self.current_height, *removed_range.start());
            if potential_start.as_u64() < self.next_uncovered_range.start().as_u64() {
                // We might be able to start earlier - compute from this potential start
                self.next_uncovered_range = self.compute_next_uncovered_range_from(potential_start);
                return;
            }
        }

        // If removed range starts after our next range, it probably doesn't affect us
        if removed_range.start().as_u64() > self.next_uncovered_range.end().as_u64() {
            return;
        }

        // Complex case: removed range intersects with area around our next range
        // Fall back to full recomputation
        self.update_next_range();
    }

    /// Internal method to compute the next uncovered range using current state
    fn compute_next_uncovered_range(&self) -> RangeInclusive<Ctx::Height> {
        self.compute_next_uncovered_range_from(self.current_height)
    }

    /// Internal method to compute the next uncovered range starting from a specific height
    fn compute_next_uncovered_range_from(
        &self,
        initial_height: Ctx::Height,
    ) -> RangeInclusive<Ctx::Height> {
        let ranges = self.get_ranges();

        // Since no pending requests end before initial_height, if any height is covered,
        // it can only be covered by exactly one range (due to disjoint property)
        // But we need to keep checking as we advance start_height
        let mut start_height = initial_height;

        // Keep advancing start_height until we find one that's not covered
        while let Some(covering_range) = ranges.iter().find(|range| range.contains(&start_height)) {
            // start_height is covered, move to right after this range
            start_height = covering_range.end().increment();
        }

        // Calculate the maximum possible end height based on batch size
        let mut end_height = start_height;
        for _ in 1..self.max_batch_size {
            end_height = end_height.increment();
        }

        // Find the first range that would limit our end height
        // All remaining ranges either start at/after initial_height or contain initial_height
        for range in &ranges {
            if range.start().as_u64() > start_height.as_u64()
                && range.start().as_u64() <= end_height.as_u64()
            {
                // This range conflicts with our desired range, limit our end to just before it
                if range.start().as_u64() > 0 {
                    end_height = range.start().decrement().unwrap_or(*range.start());
                }
                break; // Since ranges are disjoint, this is the first and only conflict
            }
        }

        start_height..=end_height
    }

    /// Get the request that contains the given height.
    /// Assumes a height cannot be in multiple pending requests.
    pub fn get_request_id_by(&self, height: Ctx::Height) -> Option<(OutboundRequestId, PeerId)> {
        self.requests
            .iter()
            .find(|(_, (range, _))| range.contains(&height))
            .map(|(request_id, (_, stored_peer_id))| (request_id.clone(), *stored_peer_id))
    }

    /// Get all ranges sorted by start height (for internal use by optimization logic)
    fn get_ranges(&self) -> Vec<RangeInclusive<Ctx::Height>> {
        let mut ranges: Vec<RangeInclusive<Ctx::Height>> = self
            .requests
            .values()
            .map(|(range, _)| range.clone())
            .collect();

        // Sort by start height for efficient processing
        ranges.sort_by_key(|range| range.start().as_u64());

        ranges
    }
}

// TODO: Add unit tests with proper Context implementation
