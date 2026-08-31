//! Which side of a card is in effect right now (#774).
//!
//! A double-sided card prints text on both faces, and only one of them is in
//! effect at a time. For a location the switch is [`Location::revealed`], and
//! `glossary/Location_Cards.md` puts it as a **reading instruction** rather
//! than as a state flag:
//!
//! > Locations enter play in an "unrevealed" state, so that the side with no
//! > shroud value and/or clue value is faceup. **Do not read the "revealed"
//! > side at this time.**
//!
//! So: **a location's `back_text` abilities apply while it is unrevealed, and
//! its front's while it is revealed.** The Parlor 01115 is the corpus's first
//! card to need the distinction — its unrevealed back reads *"The entrance to
//! the Parlor is blocked by a darkly glowing unfathomable barrier. You cannot
//! move into the Parlor."*, and act 01109b lifts it by revealing (*"The
//! barrier blocking passage into the parlor has vanished. Reveal the
//! Parlor."*).
//!
//! # Why a mechanism rather than a hardcode
//!
//! **20 locations in Core + Dunwich carry `back_text`, and every one of the 20
//! is about movement.** Nineteen are movement *restrictions*, and they split
//! seven / twelve:
//!
//! - **Seven unconditional** — Parlor 01115, Dormitories 02052, Faculty Offices
//!   02054/02055, Alchemy Labs 02057, Museum Halls 02127, Ascending Path 02283
//!   — each a bare *"You cannot move into X."* that needs nothing beyond this
//!   module and a `Trigger::Constant` `Restrict`.
//! - **Twelve conditional** — Train Car 02167-02174, Engine Car 02175-02177,
//!   The Edge of the Universe 02321 — each gating on discovered clues, which a
//!   constant `Restrict` cannot carry. That is `#821`, and it does not block
//!   this.
//!
//! The twentieth, Sentinel Peak 02284, is the one that is not a restriction:
//! *"As an additional cost to move to Sentinel Peak, the investigators must
//! spend 2 \[`per_investigator`\] clues, as a group"* — a movement **cost**, which
//! is `Restriction::ExtraActionCost`'s shape rather than a block.
//!
//! One caveat on the seven, since it is the reason the survey is a survey and
//! not a licence: **02127's back carries a second clause this module does not
//! serve** — *"Museum Entrance gains: '\[action\]: Test \[combat\] (5) to
//! attempt to break down the door to the Museum. If you are successful,
//! immediately advance to Act 1b.'"* — a granted ability, which is the *other*
//! thing this module now answers (below), and which works without a special
//! case: its back grants to a different location, and the side choice is
//! already inside [`printed_in_effect`]. Its *restriction* clause is
//! unconditional; the card is not.
//!
//! # Why not "unrevealed ⇒ unenterable"
//!
//! That is not a rule — the same entry makes entry the thing that *reveals*:
//! *"The first time a location is entered by an investigator, that location is
//! revealed by turning it to its other side and placing a number of clues on
//! it equal to its clue value."* So the Attic 01113 and the Cellar 01114 enter
//! play unrevealed and are revealed **by** being entered
//! (`dispatch::actions::resume_move_enter`, which reveals before the
//! entered-location Forced window, so their front-side Forced abilities see a
//! revealed location). Only a location that prints a barrier on its back has
//! one.
//!
//! # Why every reader funnels through here
//!
//! An ability is named across a suspension by an [`AbilityAddress`] — a
//! candidate minted by a scan is re-resolved by address when it fires. Front
//! and back are different vectors, so a scan that picks a side and a resolve
//! that picks the other would fire the wrong ability. One choice, made in one
//! place, is what keeps the address meaningful.
//!
//! # Granted abilities (#772)
//!
//! Since #772 this module answers a second question, and it is the same
//! question: *what abilities does this card have right now?* A card can have
//! abilities that are not printed on it — `glossary/Gains.md`: *"If a card
//! gains a characteristic (such as an icon, a trait, a keyword, or ability
//! text), the card functions as if it possesses the gained characteristic."*,
//! and *"'Gained' characteristics are not considered to be 'printed' on the
//! card."*
//!
//! So [`for_source`] returns the side-in-effect **printed** abilities plus
//! whatever the board **grants**, and the grant sweep ([`granted_to`]) is the
//! walk `modified_value::sweep` already does over the seven in-play
//! collections, matching `Effect::Grant` where that one matches
//! `Effect::Modify`. The sweep itself reads [`printed_in_effect`], not
//! [`for_source`] — a grant cannot be granted, which is what leaves the
//! addressing with a fixed vector to index. `docs/adr/0014-a-granted-ability-is-a-constant-effect-swept-off-the-board.md`
//! has the argument.
//!
//! [`Location::revealed`]: crate::state::Location::revealed

use crate::card_registry::{self, CardRegistry};
use crate::dsl::{Ability, Condition, Effect, GrantTarget, Trigger};
use crate::state::{
    AbilityAddress, AbilitySource, CandidateSource, CardCode, GameState, InvestigatorId, LocationId,
};

/// The abilities in effect on the location `id`: its back's while it is
/// unrevealed, its front's while it is revealed.
///
/// `None` when no registry is installed, when `id` is not in play, or when
/// the side in effect implements nothing — the same *"this card implements
/// nothing"* signal `abilities_for` gives, kept so the callers that reject on
/// it keep their reason. [`location_abilities_or_empty`] is the form for
/// callers that only want to scan.
#[must_use]
pub(crate) fn location_abilities(state: &GameState, id: LocationId) -> Option<Vec<Ability>> {
    location_abilities_with(state, card_registry::current()?, id)
}

/// [`location_abilities`] against an explicitly supplied registry.
///
/// The installed registry is a process-global `OnceLock`, but
/// [`modified_value`](crate::engine::modified_value) takes its registry **by
/// argument** — a modified value is a pure query over `(state, registry)`, and
/// its unit tests pass a mock rather than installing one. So the whole funnel
/// has a registry-taking core with a global-reading wrapper over it, rather
/// than two ways to find a card's abilities.
#[must_use]
pub(crate) fn location_abilities_with(
    state: &GameState,
    reg: &CardRegistry,
    id: LocationId,
) -> Option<Vec<Ability>> {
    let location = state.locations.get(&id)?;
    let lookup = if location.revealed {
        reg.abilities_for
    } else {
        reg.back_abilities_for
    };
    lookup(&location.code)
}

/// [`location_abilities`] with the three "nothing to scan" cases folded into
/// the empty vector, for callers that iterate rather than reject.
#[must_use]
pub(crate) fn location_abilities_or_empty(state: &GameState, id: LocationId) -> Vec<Ability> {
    location_abilities(state, id).unwrap_or_default()
}

/// The abilities in effect on the card behind `source`: the ones **printed**
/// on the side of it that is in effect, plus the ones other cards **grant** it.
///
/// Returns `None` when the card implements nothing *and* is granted nothing —
/// preserving `abilities_for`'s *"this card implements nothing"* signal for the
/// callers that reject on it. Since #772 the `None` means *"no printed **and**
/// no granted abilities"*, which is the same rejection those callers wanted.
///
/// Each ability comes paired with its [`AbilityAddress`] — how to name it again
/// after a suspension. Callers scan the pairs and carry the address, never the
/// position: the merged vector is a function of game state, so a position in it
/// is not an identity (ADR 0014).
#[must_use]
pub(crate) fn for_source(
    state: &GameState,
    source: AbilitySource,
    code: &CardCode,
) -> Option<Vec<(AbilityAddress, Ability)>> {
    for_source_with(state, card_registry::current()?, source, code)
}

/// [`for_source`] against an explicitly supplied registry — see
/// [`location_abilities_with`] for why the funnel has this shape.
#[must_use]
pub(crate) fn for_source_with(
    state: &GameState,
    reg: &CardRegistry,
    source: AbilitySource,
    code: &CardCode,
) -> Option<Vec<(AbilityAddress, Ability)>> {
    let printed = printed_in_effect(state, reg, source, code);
    let granted = granted_to(state, reg, source, code);
    if printed.is_none() && granted.is_empty() {
        return None;
    }
    let mut out = addressed_as_printed(printed.unwrap_or_default());
    out.extend(granted);
    Some(out)
}

/// [`for_source`] for a [`CandidateSource`]. A [`Hand`](CandidateSource::Hand)
/// candidate is a card being *played*, never a board card with two faces, so
/// it reads the front — and nothing grants to a card in hand, which is not in
/// play, so every address it yields is [`AbilityAddress::Printed`].
#[must_use]
pub(crate) fn for_candidate_source(
    state: &GameState,
    source: CandidateSource,
    code: &CardCode,
) -> Option<Vec<(AbilityAddress, Ability)>> {
    match source {
        CandidateSource::Ability(source) => for_source(state, source, code),
        CandidateSource::Hand => Some(addressed_as_printed(
            (card_registry::current()?.abilities_for)(code)?
        )),
    }
}

/// Pair each of a card's printed abilities with the address that names it.
#[must_use]
fn addressed_as_printed(abilities: Vec<Ability>) -> Vec<(AbilityAddress, Ability)> {
    abilities
        .into_iter()
        .enumerate()
        .map(|(idx, ability)| {
            (
                AbilityAddress::Printed(u8::try_from(idx).unwrap_or(u8::MAX)),
                ability,
            )
        })
        .collect()
}

/// **The one place an [`AbilityAddress`] becomes an [`Ability`].**
///
/// Re-derives rather than remembers: a granted ability whose granter has left
/// play, or whose condition has flipped, is simply not in the list any more and
/// resolves to `None`. A candidate holding such an address lapses through
/// machinery that already exists (`lapse_reason`), and a resolve path that
/// re-looks-up gets the ability the address *names* rather than whatever
/// currently sits at some position.
#[must_use]
pub(crate) fn resolve(
    state: &GameState,
    source: CandidateSource,
    code: &CardCode,
    address: &AbilityAddress,
) -> Option<Ability> {
    for_candidate_source(state, source, code)?
        .into_iter()
        .find(|(candidate, _)| candidate == address)
        .map(|(_, ability)| ability)
}

/// The abilities **printed** on the side of `source` that is in effect.
///
/// Private, and the grant sweep calls *this* rather than [`for_source`]. Two
/// things follow, and ADR 0014 leans on both: **a grant cannot itself be
/// granted**, so there is no fixed point to iterate to and no termination
/// argument to make; and an [`AbilityAddress::Granted`] indexes a vector that
/// is a pure function of `(code, side)`, which is what makes the address sound.
///
/// `TODO(#829)`: a granted grant does not apply. Promoting it means a bounded
/// fixed point and an address that can name a chain, and it wants a card that
/// prints one.
#[must_use]
fn printed_in_effect(
    state: &GameState,
    reg: &CardRegistry,
    source: AbilitySource,
    code: &CardCode,
) -> Option<Vec<Ability>> {
    if let AbilitySource::Location(id) = source {
        return location_abilities_with(state, reg, id);
    }
    (reg.abilities_for)(code)
}

/// **The grant sweep**: every ability the board currently grants the card
/// behind `source`.
///
/// Walks the same seven in-play collections `modified_value::sweep` does,
/// matching a **bare** `Effect::Grant` under [`Trigger::Constant`] where that
/// one matches a bare `Effect::Modify`. A grant wrapped in an `Effect::If` is
/// invisible here, deliberately and for the reason the variant's own
/// doc-comment gives.
///
/// The walk order — investigators, then locations with their attachments and
/// their at-location cards, then enemies with their attachments, then the
/// current act and agenda — is the sweep's order, and both maps are ordered, so
/// the granted abilities land in a deterministic order. Nothing depends on that
/// order for identity (an address names the granter, not a position), but a
/// stable enumeration keeps the turn menu stable between reads.
#[must_use]
fn granted_to(
    state: &GameState,
    reg: &CardRegistry,
    recipient: AbilitySource,
    recipient_code: &CardCode,
) -> Vec<(AbilityAddress, Ability)> {
    // Who "you" is for the recipient, and `None` when nobody controls it —
    // Lita Chantler 01117 sitting in the Parlor is the case that exists. A
    // condition needing a "you" does not hold against `None`; a board-global
    // one is answered anyway. (ADR 0014.)
    let you = controller_of(state, recipient);
    let mut out = Vec::new();
    let mut visit = |granter: AbilitySource, granter_code: &CardCode| {
        let Some(abilities) = printed_in_effect(state, reg, granter, granter_code) else {
            return;
        };
        for (idx, ability) in abilities.iter().enumerate() {
            if ability.trigger != Trigger::Constant {
                continue;
            }
            let Effect::Grant {
                to,
                condition,
                abilities: granted,
            } = &ability.effect
            else {
                continue;
            };
            let addressed = match to {
                GrantTarget::SelfCard => granter == recipient,
                GrantTarget::Card(code) => code.as_str() == recipient_code.as_str(),
            };
            if !addressed {
                continue;
            }
            if let Some(condition) = condition {
                if !condition_holds(state, condition, you, recipient) {
                    continue;
                }
            }
            for (sub, granted_ability) in granted.iter().enumerate() {
                out.push((
                    AbilityAddress::Granted {
                        granter: granter_code.clone(),
                        ability: u8::try_from(idx).unwrap_or(u8::MAX),
                        sub: u8::try_from(sub).unwrap_or(u8::MAX),
                    },
                    granted_ability.clone(),
                ));
            }
        }
    };

    for inv in state.investigators.values() {
        for card in inv.controlled_card_instances() {
            visit(AbilitySource::InPlay(card.instance_id), &card.code);
        }
    }
    for (id, location) in &state.locations {
        visit(AbilitySource::Location(*id), &location.code);
        for card in location
            .attachments
            .iter()
            .chain(location.cards_at_location.iter())
        {
            visit(AbilitySource::InPlay(card.instance_id), &card.code);
        }
    }
    for (id, enemy) in &state.enemies {
        visit(AbilitySource::Enemy(*id), &enemy.code);
        for att in &enemy.attachments {
            visit(AbilitySource::InPlay(att.instance_id), &att.code);
        }
    }
    if let Some(act) = state.act_deck.get(state.act_index) {
        visit(AbilitySource::Act, &act.code);
    }
    if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
        visit(AbilitySource::Agenda, &agenda.code);
    }
    out
}

/// The investigator controlling the card behind `source`, or `None` when
/// nobody does.
///
/// `glossary/Ownership_and_Control.md` splits control from ownership, and this
/// is the control half: a location, an enemy, the act, the agenda, and a card
/// put into play *at* a location are all controlled by the scenario rather than
/// by a player.
#[must_use]
fn controller_of(state: &GameState, source: AbilitySource) -> Option<InvestigatorId> {
    let instance = source.instance()?;
    state
        .investigators
        .values()
        .find(|inv| {
            inv.controlled_card_instances()
                .any(|card| card.instance_id == instance)
        })
        .map(|inv| inv.id)
}

/// Whether `condition` holds for a grant whose recipient's controller is `you`.
///
/// Folds the *"a condition needing a 'you' against `None` does not hold"* rule
/// through [`try_condition`]: an unanswerable condition is a condition that
/// does not hold, never one that silently holds.
#[must_use]
fn condition_holds(
    state: &GameState,
    condition: &Condition,
    you: Option<InvestigatorId>,
    source: AbilitySource,
) -> bool {
    try_condition(state, condition, you, source).unwrap_or(false)
}

/// [`condition_holds`]'s three-valued core: `None` means *"this condition needs
/// a 'you' and there is none"*.
///
/// Three-valued rather than two because *"cannot be asked"* and *"asked and
/// false"* are the same answer **only while no condition inverts another**. A
/// `Condition::Not`-style combinator would have to propagate the `None` rather
/// than invert it, or a negated unanswerable condition would come back `true`;
/// keeping the shape three-valued is what leaves that trap already sprung.
fn try_condition(
    state: &GameState,
    condition: &Condition,
    you: Option<InvestigatorId>,
    source: AbilitySource,
) -> Option<bool> {
    match condition {
        // Board-global: no "you" to bind, so it is answerable for an
        // uncontrolled recipient — which is the whole case the Parlor 01115's
        // grant serves.
        Condition::ControlStatus { code, status } => {
            Some(crate::engine::evaluator::card_control_status(state, code) == *status)
        }
        other => {
            let you = you?;
            let eval_ctx =
                crate::engine::evaluator::EvalContext::for_controller_with_source(you, source);
            // An unexpressible condition is card data, not an engine invariant,
            // and a constant sweep has no rejection channel — so it does not
            // hold, exactly as the modified-value sweep skips an `IntExpr` it
            // cannot resolve rather than counting it as zero.
            Some(crate::engine::evaluator::eval_condition(state, &eval_ctx, other).unwrap_or(false))
        }
    }
}
