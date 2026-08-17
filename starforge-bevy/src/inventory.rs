//! Player inventory — port of the inventory semantics in js/player.js.

use crate::data;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Slot {
    pub item: String,
    pub n: i32,
}

pub const INV_SLOTS: usize = 36;
pub const HOTBAR: usize = 9;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Inventory {
    pub slots: Vec<Option<Slot>>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![None; INV_SLOTS],
        }
    }
}

impl Inventory {
    pub fn count_item(&self, item: &str) -> i32 {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.item == item)
            .map(|s| s.n.max(0))
            .fold(0, |total, n| total.saturating_add(n))
    }

    /// Normalize data loaded from disk to the fixed-size inventory shape.
    pub fn from_slots(slots: Vec<Option<Slot>>) -> Self {
        let mut inventory = Self::default();
        for slot in slots.into_iter().take(INV_SLOTS).flatten() {
            if slot.n > 0 {
                inventory.add_item(&slot.item, slot.n);
            }
        }
        inventory
    }

    /// Add items; merges into partial stacks (oldest first), then empty slots.
    /// Returns the number actually added.
    pub fn add_item(&mut self, item: &str, n: i32) -> i32 {
        if n <= 0 {
            return 0;
        }
        let max_stack = data::item_by_key(item).map(|i| i.stack).unwrap_or(250);
        let mut remaining = n;
        // partial stacks first
        for slot in self.slots.iter_mut() {
            if remaining <= 0 {
                break;
            }
            if let Some(s) = slot
                && s.item == item
            {
                let current = s.n.max(0).min(max_stack);
                let add = (max_stack - current).min(remaining);
                s.n = current + add;
                remaining -= add;
            }
        }
        // empty slots
        for slot in self.slots.iter_mut() {
            if remaining <= 0 {
                break;
            }
            if slot.is_none() {
                let add = max_stack.min(remaining);
                *slot = Some(Slot {
                    item: item.to_string(),
                    n: add,
                });
                remaining -= add;
            }
        }
        n - remaining
    }

    /// Remove up to n of an item from the tail backwards; returns true if enough existed.
    pub fn remove_item(&mut self, item: &str, n: i32) -> bool {
        if n < 0 {
            return false;
        }
        if n == 0 {
            return true;
        }
        if self.count_item(item) < n {
            return false;
        }
        let mut remaining = n;
        for i in (0..self.slots.len()).rev() {
            if remaining <= 0 {
                break;
            }
            if let Some(s) = &mut self.slots[i]
                && s.item == item
            {
                let take = s.n.max(0).min(remaining);
                s.n -= take;
                remaining -= take;
                if s.n <= 0 {
                    self.slots[i] = None;
                }
            }
        }
        true
    }

    pub fn has_items(&self, costs: &[(&str, i32)]) -> bool {
        costs
            .iter()
            .all(|(item, n)| *n >= 0 && self.count_item(item) >= *n)
    }

    pub fn pay_items(&mut self, costs: &[(&str, i32)]) -> bool {
        if !self.has_items(costs) {
            return false;
        }
        for (item, n) in costs {
            self.remove_item(item, *n);
        }
        true
    }

    /// Sort storage slots 9..36 only (merge & compact), hotbar untouched.
    pub fn sort_storage(&mut self) {
        if self.slots.len() <= HOTBAR {
            return;
        }
        let mut totals: Vec<(String, i32)> = Vec::new();
        let storage_end = self.slots.len().min(INV_SLOTS);
        for i in HOTBAR..storage_end {
            if let Some(s) = &self.slots[i] {
                if let Some(t) = totals.iter_mut().find(|(item, _)| *item == s.item) {
                    t.1 += s.n;
                } else {
                    totals.push((s.item.clone(), s.n));
                }
            }
        }
        let mut idx = HOTBAR;
        for (item, mut n) in totals {
            let max_stack = data::item_by_key(&item).map(|i| i.stack).unwrap_or(250);
            while n > 0 {
                let take = n.min(max_stack);
                self.slots[idx] = Some(Slot {
                    item: item.clone(),
                    n: take,
                });
                n -= take;
                idx += 1;
                if idx >= storage_end {
                    break;
                }
            }
        }
        for i in idx..storage_end {
            self.slots[i] = None;
        }
    }

    /// Capacity remaining for an item.
    pub fn room_for(&self, item: &str) -> i32 {
        let max_stack = data::item_by_key(item).map(|i| i.stack).unwrap_or(250);
        let mut room = 0;
        for s in &self.slots {
            match s {
                Some(s) if s.item == item => room += (max_stack - s.n.max(0)).max(0),
                None => room += max_stack,
                _ => {}
            }
        }
        room
    }
}
