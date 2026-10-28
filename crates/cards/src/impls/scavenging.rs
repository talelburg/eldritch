//! Scavenging (Survivor talent asset, 01073).
//!
//! ```text
//! Talent.
//! [reaction] After you successfully investigate by 2 or more, exhaust
//!   Scavenging: Choose an [[Item]] card in your discard pile and add it to
//!   your hand.
//! ```
//!
//! **Cell: the `after` cell of the `SkillTestResolved` condition**, narrowed to
//! a successful Investigate *"by 2 or more"* by the pattern's
//! [`MarginScope`]. *"exhaust Scavenging"* is the cost, paid as the reaction
//! initiates.
//!
//! The ruling (<https://arkhamdb.com/card/01073>): *"Success is determined in
//! Step 6 of the Skill Test Timing, and committed cards get discarded in Step
//! 8, so you cannot use Scavenging to get back an **Item** card that you've
//! just committed to the same investigation attempt."* That falls out of the
//! timing: the after cell runs at ST.6, while a committed card is still in
//! limbo, so it is not in the discard pile to be chosen.
//!
//! **Not offered with no Item in the discard pile.** The effect is a native, so
//! the reaction scan cannot see that it would change nothing; an eligibility
//! condition says so instead, as Cover Up 01007's does. Otherwise the player
//! could exhaust Scavenging for nothing.
//!
//! **The choice** is [`resolve_card_choice`](engine::resolve_card_choice),
//! labelled by card name and un-anchored: a discard-pile card has no board
//! surface for an option to sit on. The native asks before it writes, then
//! moves the chosen card with
//! [`NativeCx::add_discarded_card_to_hand`]. A lone Item is taken without a
//! prompt.

use card_dsl::dsl::{
    self, Ability, Cost, EventPattern, EventTiming, MarginScope, SkillTestKind, TestOutcome,
    TestedLocationScope,
};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx};
use game_core::state::{CardCode, GameState, InvestigatorId};

use crate::generated::traits::ITEM;
use crate::impls::CardRecord;

/// `ArkhamDB` code for Scavenging (original-Core printing).
pub const CODE: &str = "01073";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities)
    .effects(&[(TAKE_ITEM, take_item)])
    .conditions(&[(HAS_ITEM, has_item)]);

/// Native tag: choose an Item in the discard pile and add it to hand.
const TAKE_ITEM: &str = "01073:take-item";
/// Native-condition tag: the controller's discard pile holds an Item.
const HAS_ITEM: &str = "01073:has-item";

/// "`[reaction]` After you successfully investigate by 2 or more, exhaust
/// Scavenging: Choose an \[\[Item\]\] card in your discard pile and add it to
/// your hand."
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::SkillTestResolved {
            outcome: TestOutcome::Success,
            kind: Some(SkillTestKind::Investigate),
            by_controller: true,
            tested_location: TestedLocationScope::Any,
            margin: Some(MarginScope::at_least(2)),
        },
        EventTiming::After,
        dsl::native(TAKE_ITEM),
    )
    .with_costs(vec![Cost::Exhaust])
    .with_eligibility(dsl::native_condition(HAS_ITEM))]
}

/// The Items in `investigator`'s discard pile, as `(index into the pile,
/// code)`, in pile order.
fn items_in_discard(state: &GameState, investigator: InvestigatorId) -> Vec<(usize, CardCode)> {
    state
        .investigators
        .get(&investigator)
        .map_or_else(Vec::new, |inv| {
            inv.discard
                .iter()
                .enumerate()
                .filter(|(_, code)| crate::has_trait(code.as_str(), ITEM))
                .map(|(i, code)| (i, code.clone()))
                .collect()
        })
}

fn has_item(state: &GameState, ctx: &EvalContext) -> bool {
    !items_in_discard(state, ctx.controller).is_empty()
}

fn take_item(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let items = items_in_discard(cx.state(), ctx.controller);
    let codes: Vec<CardCode> = items.iter().map(|(_, code)| code.clone()).collect();
    match engine::resolve_card_choice(
        cx.state(),
        cx.cards(),
        ctx,
        &codes,
        "Scavenging: choose an Item to add to your hand",
    ) {
        Grounded::Picked(i) => {
            cx.add_discarded_card_to_hand(ctx.controller, items[i].0);
            EngineOutcome::Done
        }
        Grounded::Empty => EngineOutcome::Done,
        Grounded::Suspend(outcome) => outcome,
    }
}
