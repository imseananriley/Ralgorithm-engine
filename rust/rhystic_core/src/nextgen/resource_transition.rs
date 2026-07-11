use super::{ManaPool, PackedState, PaymentPlan, Zone};

pub fn apply_payment_plan(state: PackedState, plan: PaymentPlan) -> Option<PackedState> {
    let consumption = plan.consumption;
    if !consumption.tapped.is_subset(state.battlefield)
        || !consumption.sacrificed.is_subset(state.battlefield)
        || !consumption.exiled.is_subset(state.hand)
        || !consumption.tapped.intersect(state.tapped).is_empty()
        || !consumption
            .tapped
            .intersect(consumption.sacrificed)
            .is_empty()
    {
        return None;
    }

    let mut next = state;
    next.tapped = next.tapped.union(consumption.tapped);
    for slot in consumption.sacrificed.iter() {
        if !next.move_card(slot, Zone::Battlefield, Zone::Graveyard) {
            return None;
        }
    }
    for slot in consumption.exiled.iter() {
        if !next.move_card(slot, Zone::Hand, Zone::Exile) {
            return None;
        }
    }
    next.mana = plan.leftover.pack();
    next.all_zones_disjoint().then_some(next)
}

#[allow(dead_code)]
fn unpack_state_mana(state: PackedState) -> ManaPool {
    ManaPool::unpack(state.mana)
}
