use serde::{Deserialize, Serialize};

use super::{CardMask, PackedLibrary, SlotId};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Zone {
    Hand,
    Battlefield,
    Graveyard,
    Exile,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackedState {
    pub hand: CardMask,
    pub battlefield: CardMask,
    pub graveyard: CardMask,
    pub exile: CardMask,
    pub tapped: CardMask,
    pub fresh: CardMask,
    pub library: PackedLibrary,
    pub mana: u32,
    pub flags: u32,
    pub counters: u32,
}

impl PackedState {
    pub fn zone(self, zone: Zone) -> CardMask {
        match zone {
            Zone::Hand => self.hand,
            Zone::Battlefield => self.battlefield,
            Zone::Graveyard => self.graveyard,
            Zone::Exile => self.exile,
        }
    }

    pub fn move_card(&mut self, slot: SlotId, from: Zone, to: Zone) -> bool {
        if !self.zone(from).contains(slot) {
            return false;
        }
        self.zone_mut(from).remove(slot);
        self.zone_mut(to).insert(slot);
        if to != Zone::Battlefield {
            self.tapped.remove(slot);
            self.fresh.remove(slot);
        }
        true
    }

    pub fn draw(&mut self, slot: SlotId) -> bool {
        if !self.library.draw(slot) {
            return false;
        }
        self.hand.insert(slot)
    }

    pub fn all_zones_disjoint(self) -> bool {
        let zones = [
            self.hand,
            self.battlefield,
            self.graveyard,
            self.exile,
            self.library.cards(),
        ];
        for left in 0..zones.len() {
            for right in left + 1..zones.len() {
                if !zones[left].intersect(zones[right]).is_empty() {
                    return false;
                }
            }
        }
        self.tapped.is_subset(self.battlefield) && self.fresh.is_subset(self.battlefield)
    }

    fn zone_mut(&mut self, zone: Zone) -> &mut CardMask {
        match zone {
            Zone::Hand => &mut self.hand,
            Zone::Battlefield => &mut self.battlefield,
            Zone::Graveyard => &mut self.graveyard,
            Zone::Exile => &mut self.exile,
        }
    }
}
