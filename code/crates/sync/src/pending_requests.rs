use std::cmp::max;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use malachitebft_core_types::{Context, Height};
use malachitebft_peer::PeerId;
use tracing::error;

use crate::OutboundRequestId;

/// Manages pending sync requests with optimized range operations.
///
/// Maintains an invariant that `next_uncovered_range` is always up-to-date and represents
/// the next range that should be requested (with the smallest start height).
///
/// The start of `next_uncovered_range` effectively serves as the current sync height.
///
/// Assumptions for optimization:
/// 1. All ranges are disjoint (no overlapping requests)
/// 2. No pending requests end before the initial sync height
#[derive(Debug)]
pub struct PendingRequests<Ctx: Context> {
    /// Map of request ID to (range, peer_id)
    requests: BTreeMap<OutboundRequestId, (RangeInclusive<Ctx::Height>, PeerId)>,

    /// Maximum batch size for ranges
    max_batch_size: u64,

    /// Pre-computed next uncovered range (always up-to-date)
    /// The start of this range is the effective "current sync height"
    next_uncovered_range: RangeInclusive<Ctx::Height>,

    /// Control field: tracks the highest height that has been validated/removed
    /// This ensures monotonic progress and validates consistency
    last_validated_height: Ctx::Height,
}

impl<Ctx: Context> PendingRequests<Ctx> {
    pub fn new(initial_height: Ctx::Height, max_batch_size: u64) -> Self {
        let max_batch_size = max(1, max_batch_size);

        // Compute initial next uncovered range
        let end_height = initial_height.increment_by(max_batch_size - 1);
        let next_uncovered_range = initial_height..=end_height;

        Self {
            requests: BTreeMap::new(),
            max_batch_size,
            next_uncovered_range,
            // Initialize to one less than initial height, or initial height if can't decrement
            last_validated_height: initial_height.decrement().unwrap_or(initial_height),
        }
    }

    /// Get the current effective sync height (start of next uncovered range)
    pub fn current_sync_height(&self) -> Ctx::Height {
        *self.next_uncovered_range.start()
    }

    /// Remove all pending requests up to and including the given height
    ///
    /// This method removes all pending requests where end_range <= height,
    /// typically called when consensus decides on a height and those ranges are no longer needed.
    ///
    /// If height < last_validated_height, logs an error and uses last_validated_height instead.
    pub fn remove_requests_up_to(&mut self, height: Ctx::Height) {
        // Handle non-monotonic progress gracefully
        if height.as_u64() < self.last_validated_height.as_u64() {
            error!(
                height = height.as_u64(),
                last_validated_height = self.last_validated_height.as_u64(),
                "Non-monotonic progress in remove_requests_up_to: height < last_validated_height. Using last_validated_height instead."
            );
            return;
        }
        // Drop all pending requests that end at or before the effective height
        self.requests.retain(|_, (range, _)| {
            // Keep the request if it ends after the effective height
            range.end().as_u64() > height.as_u64()
        });

        // Update control field to track progress
        self.last_validated_height = height;

        // Update to start syncing from the next height after the effective height
        let new_sync_height = self.last_validated_height.increment();
        let new_range = self.compute_next_uncovered_range_from(new_sync_height);
        self.set_next_uncovered_range(new_range);
    }

    /// Private method to set next_uncovered_range with validation
    ///
    /// # Panics
    /// Panics if the new range start <= last_validated_height (consistency violation)
    fn set_next_uncovered_range(&mut self, new_range: RangeInclusive<Ctx::Height>) {
        // Validate consistency before setting
        assert!(
            new_range.start().as_u64() > self.last_validated_height.as_u64(),
            "Consistency violation: attempting to set next_uncovered_range.start() {} <= last_validated_height {}",
            new_range.start().as_u64(),
            self.last_validated_height.as_u64()
        );

        self.next_uncovered_range = new_range;
    }

    /// Insert a new pending request
    pub fn insert(
        &mut self,
        request_id: OutboundRequestId,
        range: RangeInclusive<Ctx::Height>,
        peer_id: PeerId,
    ) {
        self.requests.insert(request_id, (range.clone(), peer_id));
        // Update the next uncovered range based on the inserted range
        self.update_next_range_after_insert(&range);
    }

    /// Remove a pending request
    pub fn remove(
        &mut self,
        request_id: &OutboundRequestId,
    ) -> Option<(RangeInclusive<Ctx::Height>, PeerId)> {
        let result = self.requests.remove(request_id);
        if let Some((removed_range, _)) = &result {
            // Update the next uncovered range based on the removed range
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

    /// Get the next uncovered range that should be requested.
    ///
    /// This is always up-to-date and represents the next range with the smallest start height
    /// that is not covered by any pending request.
    pub fn next_uncovered_range(&self) -> RangeInclusive<Ctx::Height> {
        self.next_uncovered_range.clone()
    }

    /// Update the next uncovered range based on current state.
    ///
    /// This method recalculates the next uncovered range and should be called
    /// whenever requests are added, removed, or the current height changes.
    fn update_next_range(&mut self) {
        let new_range = self.compute_next_uncovered_range();
        self.set_next_uncovered_range(new_range);
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
            let new_range = self.compute_next_uncovered_range_from(new_start);
            self.set_next_uncovered_range(new_range);
            return;
        }

        // Edge case: inserted range might affect our current sync height, need full recompute
        if inserted_range.contains(self.next_uncovered_range.start()) {
            let new_range =
                self.compute_next_uncovered_range_from(*self.next_uncovered_range.start());
            self.set_next_uncovered_range(new_range);
            return;
        }

        // No conflict, keep current next range
    }

    /// Smart update after removing a range - uses knowledge of the specific removed range  
    fn update_next_range_after_remove(&mut self, removed_range: &RangeInclusive<Ctx::Height>) {
        // Quick check: if removed range ends before our current next range, we might start earlier
        if removed_range.end().as_u64() < self.next_uncovered_range.start().as_u64() {
            // Check if we can now start from the removed range
            let potential_start = max(*self.next_uncovered_range.start(), *removed_range.start());
            if potential_start.as_u64() < self.next_uncovered_range.start().as_u64() {
                // We might be able to start earlier - compute from this potential start
                let new_range = self.compute_next_uncovered_range_from(potential_start);
                self.set_next_uncovered_range(new_range);
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
        self.compute_next_uncovered_range_from(*self.next_uncovered_range.start())
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
        let mut end_height = start_height.increment_by(self.max_batch_size - 1);

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
