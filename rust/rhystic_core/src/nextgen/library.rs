use serde::{Deserialize, Serialize};

use super::{CardMask, SlotId};

pub const KNOWN_TOP_CAPACITY: usize = 4;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackedLibrary {
    unknown: CardMask,
    known_top: [SlotId; KNOWN_TOP_CAPACITY],
    known_top_len: u8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChanceDraw {
    pub slot: SlotId,
    pub numerator: u8,
    pub denominator: u8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassChanceDraw {
    pub class_id: u8,
    pub representative: SlotId,
    pub numerator: u8,
    pub denominator: u8,
}

impl PackedLibrary {
    pub const fn new(unknown: CardMask) -> Self {
        Self {
            unknown,
            known_top: [0; KNOWN_TOP_CAPACITY],
            known_top_len: 0,
        }
    }

    pub const fn unknown(self) -> CardMask {
        self.unknown
    }

    pub const fn known_top_len(self) -> usize {
        self.known_top_len as usize
    }

    pub fn push_known_top(&mut self, slot: SlotId) {
        assert!(
            self.known_top_len() < KNOWN_TOP_CAPACITY,
            "known-top stack capacity exceeded"
        );
        self.unknown.remove(slot);
        let len = self.known_top_len();
        self.known_top.copy_within(0..len, 1);
        self.known_top[0] = slot;
        self.known_top_len += 1;
    }

    pub fn remove_known_or_unknown(&mut self, slot: SlotId) -> bool {
        if self.unknown.remove(slot) {
            return true;
        }
        let len = self.known_top_len();
        let Some(index) = self.known_top[..len].iter().position(|item| *item == slot) else {
            return false;
        };
        self.known_top.copy_within(index + 1..len, index);
        self.known_top_len -= 1;
        true
    }

    pub fn shuffle_all_unknown(&mut self) {
        for index in 0..self.known_top_len() {
            self.unknown.insert(self.known_top[index]);
        }
        self.known_top_len = 0;
    }

    pub fn chance_draws(self) -> Vec<ChanceDraw> {
        if self.known_top_len > 0 {
            return vec![ChanceDraw {
                slot: self.known_top[0],
                numerator: 1,
                denominator: 1,
            }];
        }
        let denominator = self.unknown.len() as u8;
        self.unknown
            .iter()
            .map(|slot| ChanceDraw {
                slot,
                numerator: 1,
                denominator,
            })
            .collect()
    }

    pub fn class_chance_draws(self, class_by_slot: &[u8; 128]) -> Vec<ClassChanceDraw> {
        if self.known_top_len > 0 {
            let slot = self.known_top[0];
            return vec![ClassChanceDraw {
                class_id: class_by_slot[slot as usize],
                representative: slot,
                numerator: 1,
                denominator: 1,
            }];
        }

        let denominator = self.unknown.len() as u8;
        let mut counts = [0u8; 128];
        let mut representatives = [0u8; 128];
        let mut present = CardMask::EMPTY;
        for slot in self.unknown.iter() {
            let class_id = class_by_slot[slot as usize];
            counts[class_id as usize] = counts[class_id as usize].saturating_add(1);
            if present.insert(class_id) {
                representatives[class_id as usize] = slot;
            }
        }
        present
            .iter()
            .map(|class_id| ClassChanceDraw {
                class_id,
                representative: representatives[class_id as usize],
                numerator: counts[class_id as usize],
                denominator,
            })
            .collect()
    }

    pub fn draw(&mut self, slot: SlotId) -> bool {
        if self.known_top_len > 0 {
            if self.known_top[0] != slot {
                return false;
            }
            let len = self.known_top_len();
            self.known_top.copy_within(1..len, 0);
            self.known_top_len -= 1;
            return true;
        }
        self.unknown.remove(slot)
    }

    pub fn card_count(self) -> u32 {
        self.unknown.len() + u32::from(self.known_top_len)
    }

    pub fn cards(self) -> CardMask {
        let mut cards = self.unknown;
        for slot in &self.known_top[..self.known_top_len()] {
            cards.insert(*slot);
        }
        cards
    }
}

impl Default for PackedLibrary {
    fn default() -> Self {
        Self::new(CardMask::EMPTY)
    }
}
