use serde::{Deserialize, Serialize};

pub const MAX_DECK_SLOTS: usize = 128;
pub type SlotId = u8;

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardMask(u128);

impl CardMask {
    pub const EMPTY: Self = Self(0);

    pub const fn from_bits(bits: u128) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u128 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    pub const fn contains(self, slot: SlotId) -> bool {
        slot < 128 && self.0 & (1u128 << slot) != 0
    }

    pub fn insert(&mut self, slot: SlotId) -> bool {
        assert!(
            (slot as usize) < MAX_DECK_SLOTS,
            "slot exceeds packed deck capacity"
        );
        let bit = 1u128 << slot;
        let changed = self.0 & bit == 0;
        self.0 |= bit;
        changed
    }

    pub fn remove(&mut self, slot: SlotId) -> bool {
        if (slot as usize) >= MAX_DECK_SLOTS {
            return false;
        }
        let bit = 1u128 << slot;
        let changed = self.0 & bit != 0;
        self.0 &= !bit;
        changed
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    pub const fn is_subset(self, other: Self) -> bool {
        self.0 & !other.0 == 0
    }

    pub fn iter(self) -> CardMaskIter {
        CardMaskIter(self.0)
    }
}

pub struct CardMaskIter(u128);

impl Iterator for CardMaskIter {
    type Item = SlotId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0 == 0 {
            return None;
        }
        let slot = self.0.trailing_zeros() as SlotId;
        self.0 &= self.0 - 1;
        Some(slot)
    }
}

impl FromIterator<SlotId> for CardMask {
    fn from_iter<T: IntoIterator<Item = SlotId>>(iter: T) -> Self {
        let mut mask = Self::EMPTY;
        for slot in iter {
            mask.insert(slot);
        }
        mask
    }
}
