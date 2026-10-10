/* crates/mc-magpie/src/slot.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Single-value hand-off between a producer and a consumer.
///
/// Publishing overwrites any value the consumer has not taken yet, so memory use stays constant
/// no matter how far behind the consumer falls.
pub struct LatestSlot<T> {
    value: Mutex<Option<T>>,
    published: AtomicU64,
    dropped: AtomicU64,
}

impl<T> Default for LatestSlot<T> {
    fn default() -> Self {
        Self {
            value: Mutex::new(None),
            published: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        }
    }
}

impl<T> LatestSlot<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `value`, replacing a not yet consumed one. Returns `true` if a value was replaced.
    pub fn publish(&self, value: T) -> bool {
        let replaced = match self.value.lock() {
            Ok(mut slot) => slot.replace(value).is_some(),
            Err(poisoned) => poisoned.into_inner().replace(value).is_some(),
        };

        self.published.fetch_add(1, Ordering::Relaxed);
        if replaced {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }

        replaced
    }

    /// Takes the most recent value, if one was published since the last call.
    pub fn take(&self) -> Option<T> {
        match self.value.lock() {
            Ok(mut slot) => slot.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        }
    }

    /// Total number of values published so far.
    pub fn published(&self) -> u64 {
        self.published.load(Ordering::Relaxed)
    }

    /// Number of values that were overwritten before being consumed.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_returns_nothing_before_publish() {
        let slot = LatestSlot::<u32>::new();
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn newer_values_overwrite_unconsumed_ones() {
        let slot = LatestSlot::new();
        assert!(!slot.publish(1));
        assert!(slot.publish(2));
        assert!(slot.publish(3));

        assert_eq!(slot.take(), Some(3));
        assert_eq!(slot.take(), None);
        assert_eq!(slot.published(), 3);
        assert_eq!(slot.dropped(), 2);
    }

    #[test]
    fn slot_never_holds_more_than_one_value() {
        let slot = LatestSlot::new();
        for i in 0..10_000 {
            slot.publish(vec![i; 16]);
        }

        assert_eq!(slot.take(), Some(vec![9_999; 16]));
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn slot_works_across_threads() {
        let slot = std::sync::Arc::new(LatestSlot::new());
        let producer = {
            let slot = slot.clone();
            std::thread::spawn(move || {
                for i in 0..1000u32 {
                    slot.publish(i);
                }
            })
        };
        producer.join().unwrap();

        assert_eq!(slot.take(), Some(999));
    }
}
