use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

use super::{CardMask, ManaPool, PackedLibrary, SlotId, Zone, MAX_DECK_SLOTS};

pub const MAX_BATTLEFIELD_PERMANENTS: usize = 16;
pub const NO_ATTACHMENT: u8 = u8::MAX;
pub const COMMANDER_SOURCE: u8 = u8::MAX - 1;
const TOKEN_SOURCE_BASE: u8 = 128;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum TokenKind {
    Treasure = 0,
    GenericArtifact = 1,
    GenericCreature = 2,
    Copy = 3,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PermanentSource(u8);

impl PermanentSource {
    pub fn card(slot: SlotId) -> Self {
        assert!((slot as usize) < MAX_DECK_SLOTS);
        Self(slot)
    }

    pub const fn token(kind: TokenKind) -> Self {
        Self(TOKEN_SOURCE_BASE + kind as u8)
    }

    pub const fn commander() -> Self {
        Self(COMMANDER_SOURCE)
    }

    pub const fn id(self) -> u8 {
        self.0
    }

    pub const fn card_slot(self) -> Option<SlotId> {
        if self.0 < TOKEN_SOURCE_BASE {
            Some(self.0)
        } else {
            None
        }
    }

    pub const fn token_kind(self) -> Option<TokenKind> {
        match self.0 {
            128 => Some(TokenKind::Treasure),
            129 => Some(TokenKind::GenericArtifact),
            130 => Some(TokenKind::GenericCreature),
            131 => Some(TokenKind::Copy),
            _ => None,
        }
    }

    pub const fn is_commander(self) -> bool {
        self.0 == COMMANDER_SOURCE
    }

    pub const fn is_known(self) -> bool {
        self.card_slot().is_some() || self.token_kind().is_some() || self.is_commander()
    }
}

#[derive(
    Debug, Copy, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct PermanentInstance(u32);

impl PermanentInstance {
    const TAPPED: u32 = 1 << 24;
    const FRESH: u32 = 1 << 25;

    pub const fn new(source: PermanentSource) -> Self {
        Self(source.id() as u32 | ((NO_ATTACHMENT as u32) << 8))
    }

    pub const fn source(self) -> PermanentSource {
        PermanentSource((self.0 & 0xff) as u8)
    }

    pub const fn attached_to(self) -> Option<PermanentSource> {
        let source = ((self.0 >> 8) & 0xff) as u8;
        if source == NO_ATTACHMENT {
            None
        } else {
            Some(PermanentSource(source))
        }
    }

    pub const fn counters(self) -> u8 {
        ((self.0 >> 16) & 0xff) as u8
    }

    pub const fn tapped(self) -> bool {
        self.0 & Self::TAPPED != 0
    }

    pub const fn fresh(self) -> bool {
        self.0 & Self::FRESH != 0
    }

    pub const fn with_attachment(mut self, target: PermanentSource) -> Self {
        self.0 = (self.0 & !(0xff << 8)) | ((target.id() as u32) << 8);
        self
    }

    pub const fn without_attachment(mut self) -> Self {
        self.0 = (self.0 & !(0xff << 8)) | ((NO_ATTACHMENT as u32) << 8);
        self
    }

    pub const fn with_counters(mut self, counters: u8) -> Self {
        self.0 = (self.0 & !(0xff << 16)) | ((counters as u32) << 16);
        self
    }

    pub const fn with_tapped(mut self, tapped: bool) -> Self {
        if tapped {
            self.0 |= Self::TAPPED;
        } else {
            self.0 &= !Self::TAPPED;
        }
        self
    }

    pub const fn with_fresh(mut self, fresh: bool) -> Self {
        if fresh {
            self.0 |= Self::FRESH;
        } else {
            self.0 &= !Self::FRESH;
        }
        self
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermanentSet {
    entries: [PermanentInstance; MAX_BATTLEFIELD_PERMANENTS],
    len: u8,
}

impl Hash for PermanentSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.len.hash(state);
        self.as_slice().hash(state);
    }
}

impl Default for PermanentSet {
    fn default() -> Self {
        Self {
            entries: [PermanentInstance::default(); MAX_BATTLEFIELD_PERMANENTS],
            len: 0,
        }
    }
}

impl PermanentSet {
    pub const fn len(self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[PermanentInstance] {
        &self.entries[..self.len()]
    }

    pub fn insert(&mut self, permanent: PermanentInstance) -> bool {
        let len = self.len();
        if len == MAX_BATTLEFIELD_PERMANENTS {
            return false;
        }
        let index = self.as_slice().partition_point(|entry| *entry <= permanent);
        self.entries.copy_within(index..len, index + 1);
        self.entries[index] = permanent;
        self.len += 1;
        true
    }

    pub fn remove(&mut self, permanent: PermanentInstance) -> bool {
        let Ok(index) = self.as_slice().binary_search(&permanent) else {
            return false;
        };
        self.remove_index(index);
        true
    }

    pub fn remove_source(&mut self, source: PermanentSource) -> Option<PermanentInstance> {
        let index = self
            .as_slice()
            .iter()
            .position(|permanent| permanent.source() == source)?;
        Some(self.remove_index(index))
    }

    pub fn replace(&mut self, old: PermanentInstance, replacement: PermanentInstance) -> bool {
        if !self.remove(old) {
            return false;
        }
        let inserted = self.insert(replacement);
        debug_assert!(inserted);
        inserted
    }

    pub fn contains_source(&self, source: PermanentSource) -> bool {
        self.as_slice()
            .iter()
            .any(|permanent| permanent.source() == source)
    }

    pub fn card_mask(&self) -> Option<CardMask> {
        let mut cards = CardMask::EMPTY;
        for permanent in self.as_slice() {
            if let Some(slot) = permanent.source().card_slot() {
                if !cards.insert(slot) {
                    return None;
                }
            }
        }
        Some(cards)
    }

    pub fn is_canonical(&self) -> bool {
        let len = self.len as usize;
        if len > MAX_BATTLEFIELD_PERMANENTS {
            return false;
        }
        self.entries[..len]
            .windows(2)
            .all(|pair| pair[0] <= pair[1])
            && self.entries[len..]
                .iter()
                .all(|permanent| *permanent == PermanentInstance::default())
    }

    pub fn attachments_are_valid(&self) -> bool {
        self.as_slice().iter().all(|permanent| {
            permanent.source().is_known()
                && permanent.attached_to().is_none_or(|target| {
                    (target.card_slot().is_some() || target.is_commander())
                        && target != permanent.source()
                        && self.contains_source(target)
                })
        })
    }

    fn remove_index(&mut self, index: usize) -> PermanentInstance {
        let removed = self.entries[index];
        let len = self.len();
        self.entries.copy_within(index + 1..len, index);
        self.entries[len - 1] = PermanentInstance::default();
        self.len -= 1;
        removed
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum CommanderZone {
    #[default]
    Command = 0,
    Hand = 1,
    Battlefield = 2,
    Graveyard = 3,
    Exile = 4,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommanderState {
    pub zone: CommanderZone,
    pub cast_count: u8,
    pub tax: u8,
    pub flags: u8,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackedStateV2 {
    pub hand: CardMask,
    pub graveyard: CardMask,
    pub exile: CardMask,
    pub library: PackedLibrary,
    pub battlefield: PermanentSet,
    pub mana: ManaPool,
    pub flags: u32,
    pub counters: u32,
    pub commander: CommanderState,
}

impl PackedStateV2 {
    pub fn draw(&mut self, slot: SlotId) -> bool {
        if !self.library.draw(slot) {
            return false;
        }
        self.hand.insert(slot)
    }

    pub fn move_card(&mut self, slot: SlotId, from: Zone, to: Zone) -> bool {
        if matches!(from, Zone::Battlefield) || matches!(to, Zone::Battlefield) {
            return false;
        }
        if !self.zone(from).contains(slot) {
            return false;
        }
        self.zone_mut(from).remove(slot);
        self.zone_mut(to).insert(slot)
    }

    pub fn move_card_to_battlefield(
        &mut self,
        slot: SlotId,
        from: Zone,
        permanent: PermanentInstance,
    ) -> bool {
        if matches!(from, Zone::Battlefield)
            || permanent.source() != PermanentSource::card(slot)
            || !self.zone(from).contains(slot)
            || !self.battlefield.insert(permanent)
        {
            return false;
        }
        self.zone_mut(from).remove(slot);
        true
    }

    pub fn move_card_from_battlefield(&mut self, slot: SlotId, to: Zone) -> bool {
        if matches!(to, Zone::Battlefield) || self.zone(to).contains(slot) {
            return false;
        }
        let Some(permanent) = self.battlefield.remove_source(PermanentSource::card(slot)) else {
            return false;
        };
        self.detach_from(permanent.source());
        self.zone_mut(to).insert(slot)
    }

    pub fn add_token(&mut self, kind: TokenKind) -> bool {
        self.battlefield
            .insert(PermanentInstance::new(PermanentSource::token(kind)))
    }

    pub fn remove_token(&mut self, kind: TokenKind) -> bool {
        self.battlefield
            .remove_source(PermanentSource::token(kind))
            .is_some()
    }

    pub fn put_commander_on_battlefield(&mut self, tapped: bool, fresh: bool) -> bool {
        if self.commander.zone == CommanderZone::Battlefield {
            return false;
        }
        let permanent = PermanentInstance::new(PermanentSource::commander())
            .with_tapped(tapped)
            .with_fresh(fresh);
        if !self.battlefield.insert(permanent) {
            return false;
        }
        self.commander.zone = CommanderZone::Battlefield;
        true
    }

    pub fn return_commander_to_command_zone(&mut self) -> bool {
        if self
            .battlefield
            .remove_source(PermanentSource::commander())
            .is_none()
        {
            return false;
        }
        self.detach_from(PermanentSource::commander());
        self.commander.zone = CommanderZone::Command;
        true
    }

    pub fn is_valid(&self) -> bool {
        if !self.battlefield.is_canonical() {
            return false;
        }
        let Some(battlefield_cards) = self.battlefield.card_mask() else {
            return false;
        };
        let zones = [
            self.hand,
            self.graveyard,
            self.exile,
            self.library.cards(),
            battlefield_cards,
        ];
        for left in 0..zones.len() {
            for right in left + 1..zones.len() {
                if !zones[left].intersect(zones[right]).is_empty() {
                    return false;
                }
            }
        }
        let commander_on_battlefield = self
            .battlefield
            .contains_source(PermanentSource::commander());
        commander_on_battlefield == (self.commander.zone == CommanderZone::Battlefield)
            && self.battlefield.attachments_are_valid()
    }

    fn zone(&self, zone: Zone) -> CardMask {
        match zone {
            Zone::Hand => self.hand,
            Zone::Graveyard => self.graveyard,
            Zone::Exile => self.exile,
            Zone::Battlefield => self.battlefield.card_mask().unwrap_or(CardMask::EMPTY),
        }
    }

    fn zone_mut(&mut self, zone: Zone) -> &mut CardMask {
        match zone {
            Zone::Hand => &mut self.hand,
            Zone::Graveyard => &mut self.graveyard,
            Zone::Exile => &mut self.exile,
            Zone::Battlefield => panic!("battlefield requires a permanent transition"),
        }
    }

    fn detach_from(&mut self, source: PermanentSource) {
        let attached: [PermanentInstance; MAX_BATTLEFIELD_PERMANENTS] = self.battlefield.entries;
        for permanent in attached.into_iter().take(self.battlefield.len()) {
            if permanent.attached_to() == Some(source) {
                self.battlefield
                    .replace(permanent, permanent.without_attachment());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_permanent_lengths_are_rejected_without_slicing() {
        let mut state = PackedStateV2::default();
        state.battlefield.len = (MAX_BATTLEFIELD_PERMANENTS + 1) as u8;
        assert!(!state.is_valid());
    }

    #[test]
    fn self_attachments_are_invalid() {
        let source = PermanentSource::card(3);
        let mut state = PackedStateV2::default();
        state
            .battlefield
            .insert(PermanentInstance::new(source).with_attachment(source));
        assert!(!state.is_valid());
    }
}
