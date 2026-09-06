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
        Self::from_slots_with_capacity(slots, INV_SLOTS)
    }

    /// Normalize a variable-size container without rearranging valid saved slots.
    pub fn from_slots_with_capacity(slots: Vec<Option<Slot>>, capacity: usize) -> Self {
        let mut inventory = Self {
            slots: vec![None; capacity],
        };
        let mut overflow = Vec::new();
        for (index, slot) in slots.into_iter().take(capacity).enumerate() {
            if let Some(mut slot) = slot
                && slot.n > 0
                && let Some(item) = data::item_by_key(&slot.item)
            {
                let count = slot.n.min(1_000_000);
                slot.n = count.min(item.stack);
                if count > slot.n {
                    overflow.push(Slot {
                        item: slot.item.clone(),
                        n: count - slot.n,
                    });
                }
                inventory.slots[index] = Some(slot);
            }
        }
        // Reserve every saved slot before redistributing oversized stacks.
        for slot in overflow {
            inventory.add_item(&slot.item, slot.n);
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

    /// Remove up to `n` from one exact slot and return what was removed.
    pub fn take_from_slot(&mut self, index: usize, n: i32) -> Option<Slot> {
        if n <= 0 {
            return None;
        }
        let slot = self.slots.get_mut(index)?;
        let current = slot.as_mut()?;
        let take = current.n.max(0).min(n);
        if take <= 0 {
            return None;
        }
        let item = current.item.clone();
        current.n -= take;
        if current.n <= 0 {
            *slot = None;
        }
        Some(Slot { item, n: take })
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

/// Leaf plugin: inventory data/logic; plugin form keeps the 'everything is a
/// plugin' contract uniform (a pick-up system could live here later).
pub struct InventoryPlugin;

impl bevy::prelude::Plugin for InventoryPlugin {
    fn build(&self, _app: &mut bevy::prelude::App) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_inventory_preserves_hotbar_and_storage_layout() {
        let mut slots = vec![None; INV_SLOTS];
        slots[2] = Some(Slot {
            item: "iron".into(),
            n: 3,
        });
        slots[8] = Some(Slot {
            item: "copper".into(),
            n: 5,
        });
        slots[INV_SLOTS - 1] = Some(Slot {
            item: "iron".into(),
            n: 7,
        });

        let inventory = Inventory::from_slots(slots.clone());
        assert_eq!(inventory.slots, slots);
        // Character loading normalizes once while reading and again at spawn.
        assert_eq!(Inventory::from_slots(inventory.slots).slots, slots);
    }

    #[test]
    fn loaded_cargo_preserves_slot_layout() {
        let mut slots = vec![None; 48];
        slots[10] = Some(Slot {
            item: "iron".into(),
            n: 3,
        });
        slots[47] = Some(Slot {
            item: "iron".into(),
            n: 7,
        });

        let inventory = Inventory::from_slots_with_capacity(slots.clone(), 48);
        assert_eq!(inventory.slots, slots);
    }

    #[test]
    fn loaded_slots_sanitize_without_displacing_valid_stacks() {
        let max_stack = data::item_by_key("iron").unwrap().stack;
        let copper = Some(Slot {
            item: "copper".into(),
            n: 5,
        });
        let inventory = Inventory::from_slots_with_capacity(
            vec![
                Some(Slot {
                    item: "iron".into(),
                    n: max_stack + 3,
                }),
                copper.clone(),
                Some(Slot {
                    item: "iron".into(),
                    n: 0,
                }),
                Some(Slot {
                    item: "carbon".into(),
                    n: -1,
                }),
                Some(Slot {
                    item: "unknown_item".into(),
                    n: 10,
                }),
            ],
            6,
        );

        assert_eq!(inventory.slots.len(), 6);
        assert_eq!(inventory.slots[1], copper);
        assert_eq!(inventory.count_item("iron"), max_stack + 3);
        assert_eq!(inventory.count_item("copper"), 5);
        for slot in inventory.slots.iter().flatten() {
            let item = data::item_by_key(&slot.item).unwrap();
            assert!(slot.n > 0 && slot.n <= item.stack);
        }
        assert_eq!(inventory.slots.iter().flatten().count(), 3);
    }

    #[test]
    fn variable_container_preserves_requested_capacity() {
        let inventory = Inventory::from_slots_with_capacity(
            vec![Some(Slot {
                item: "iron".into(),
                n: 3,
            })],
            48,
        );
        assert_eq!(inventory.slots.len(), 48);
        assert_eq!(inventory.count_item("iron"), 3);
    }

    #[test]
    fn exact_slot_take_does_not_consume_other_stacks() {
        let mut inventory = Inventory {
            slots: vec![
                Some(Slot {
                    item: "iron".into(),
                    n: 2,
                }),
                Some(Slot {
                    item: "iron".into(),
                    n: 5,
                }),
            ],
        };
        let taken = inventory.take_from_slot(0, 1).unwrap();
        assert_eq!(taken.n, 1);
        assert_eq!(inventory.slots[0].as_ref().map(|slot| slot.n), Some(1));
        assert_eq!(inventory.slots[1].as_ref().map(|slot| slot.n), Some(5));
    }
}
