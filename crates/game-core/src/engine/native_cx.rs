//! The context a card-local native effect resolves in: read the whole state,
//! change the game only through the engine's exits.

use card_dsl::card_data::CardMetadata;
use card_dsl::dsl::Restriction;

use crate::card_registry::CardRegistry;
use crate::engine::board::{self, LeavingCard, MoveResourcesError, NotLeft, Placement};
use crate::engine::dispatch::actions::{r#move, take};
use crate::engine::dispatch::combat::MoveHorrorError;
use crate::engine::dispatch::{
    act_agenda, cards, combat, elimination, encounter, hunters, placement, reveal, set_aside,
    skill_test, threat_area,
    uses::{self, MoveUsesError, SpendUsesError},
};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::TraumaKind;
use crate::state::{
    CardCode, CardInstanceId, Deck, EnemyId, GameState, InvestigatorId, LocationId, Owner,
    SkillSubstitution, UseKind, Zone,
};

/// What a [`NativeEffectFn`](crate::card_registry::NativeEffectFn) gets: full
/// read access to the game, and a fixed set of **exits** as its only way to
/// change it.
///
/// Each exit owns the rules a raw write would have to remember: a card that
/// leaves play goes where its owner says, one move emits one event, doom is
/// not placed on an empty agenda deck, a card with uses is discarded once they
/// run out. There is no mutable state accessor and no way to push an event, so
/// a native that writes around an exit doesn't compile:
///
/// ```compile_fail,E0594
/// # use game_core::engine::{EngineOutcome, NativeCx};
/// # use game_core::engine::evaluator::EvalContext;
/// fn raw_write(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
///     cx.state().agenda_doom = 7;
///     EngineOutcome::Done
/// }
/// ```
///
/// A new way for a native to change the game is a new exit here.
///
/// # Choose before you write
///
/// A native that suspends for a choice (returns
/// [`EngineOutcome::AwaitingInput`]) is re-run from the top when the choice is
/// answered, so it must ask before it writes. Every exit marks the context as
/// written to when it is called, even when it turns out to change nothing, and
/// a native that suspends after any exit call is `Rejected` in every build. The
/// apply's snapshot rollback then undoes what it wrote (#571).
///
/// Read-only helpers, such as
/// [`resolve_grounded_choice`](crate::engine::resolve_grounded_choice),
/// [`resolve_card_choice`](crate::engine::resolve_card_choice) and
/// [`enemy_can_enter_location`](crate::engine::enemy_can_enter_location), are
/// free functions called on [`state`](Self::state), so asking never counts as
/// writing. No exit suspends by itself: one that needs a later step, such as an
/// encounter card's resolution, defers it through a continuation frame and
/// returns.
pub struct NativeCx<'a> {
    cx: Cx<'a>,
    written: bool,
}

impl<'a> NativeCx<'a> {
    /// Wrap `cx` for one native call, not yet written to.
    pub(crate) fn new(cx: Cx<'a>) -> Self {
        Self { cx, written: false }
    }

    /// Whether any exit has been called since [`new`](Self::new).
    pub(crate) fn written(&self) -> bool {
        self.written
    }

    /// The game state, read-only.
    #[must_use]
    pub fn state(&self) -> &GameState {
        self.cx.state
    }

    /// The card registry this apply runs against, for a read helper that
    /// looks a card up.
    #[must_use]
    pub fn cards(&self) -> &CardRegistry {
        self.cx.cards
    }

    /// Mark the context written and hand out the inner `Cx` for one exit.
    fn write_access(&mut self) -> &mut Cx<'a> {
        self.written = true;
        &mut self.cx
    }

    // --- Harm and defeat ---

    /// Deal `damage` + `horror` to `investigator`: the one way a native deals
    /// harm to an investigator. It is dealt the way an enemy attack's is — the
    /// player is offered the soak, and both `DamageAssigned` and `DamagePlaced`
    /// are announced — so nothing is placed when this returns.
    ///
    /// **Return its outcome.** The deal runs on a frame above everything the
    /// native leaves beneath it, so work written after this call would run
    /// before the harm lands (ADR 0003).
    #[must_use = "deal_harm only pushes the deal frame: return its outcome (ADR 0003)"]
    pub fn deal_harm(
        &mut self,
        investigator: InvestigatorId,
        damage: u8,
        horror: u8,
    ) -> EngineOutcome {
        combat::deal_harm(self.write_access(), investigator, damage, horror)
    }

    /// Defeat `investigator` by a card ability and run elimination. A no-op
    /// for an investigator who is not active.
    pub fn defeat_investigator(&mut self, investigator: InvestigatorId) {
        elimination::defeat_investigator(self.write_access(), investigator);
    }

    /// `investigator` suffers `amount` trauma of `kind`. Emits
    /// [`Event::TraumaSuffered`](crate::event::Event::TraumaSuffered); nothing
    /// records it yet (#766).
    pub fn suffer_trauma(&mut self, investigator: InvestigatorId, kind: TraumaKind, amount: u8) {
        elimination::suffer_trauma(self.write_access(), investigator, kind, amount);
    }

    /// Deal `amount` damage to `enemy`, defeating it if that is lethal. `by`
    /// names the investigator dealing it, if any.
    pub fn deal_damage_to_enemy(&mut self, enemy: EnemyId, amount: u8, by: Option<InvestigatorId>) {
        combat::deal_damage_to_enemy(self.write_access(), enemy, amount, by);
    }

    // --- Cards entering play ---

    /// Put the set-aside card `code` into play, at the location coded `at`
    /// where its cardtype needs one, under `owner`. `Rejected`, changing
    /// nothing, when the card is not set aside or cannot go where `at` says.
    pub fn put_set_aside_card_into_play(
        &mut self,
        code: &str,
        at: Option<&str>,
        owner: Owner,
    ) -> EngineOutcome {
        set_aside::put_set_aside_card_into_play(self.write_access(), code, at, owner)
    }

    /// Reveal `location` if it is unrevealed, placing its printed clues.
    pub fn reveal_location(&mut self, location: LocationId) {
        reveal::reveal_location(self.write_access(), location);
    }

    /// Attach `code` to `location` under `owner`, returning its new instance
    /// id, or `None` when the location is not in play.
    pub fn attach_to_location(
        &mut self,
        location: LocationId,
        code: CardCode,
        owner: Owner,
    ) -> Option<CardInstanceId> {
        threat_area::attach_to_location(self.write_access(), location, code, owner)
    }

    /// Put `code` into `investigator`'s threat area under `owner`, returning
    /// its new instance id, or `None` when the investigator is not in the game.
    pub fn place_in_threat_area(
        &mut self,
        investigator: InvestigatorId,
        code: CardCode,
        owner: Owner,
    ) -> Option<CardInstanceId> {
        threat_area::place_in_threat_area(self.write_access(), investigator, code, owner)
    }

    // --- Movement and placement ---

    /// Move `enemy` to `to` and, if it is unengaged, engage it there.
    pub fn relocate_enemy(&mut self, enemy: EnemyId, to: LocationId) {
        hunters::relocate_enemy(self.write_access(), enemy, to);
    }

    /// Place `investigator` at `location`. A placement, not a move: no move
    /// triggers fire. Emits
    /// [`Event::InvestigatorMoved`](crate::event::Event::InvestigatorMoved)
    /// only when the investigator was at a location and it differs from
    /// `location`.
    pub fn place_investigator(&mut self, investigator: InvestigatorId, location: LocationId) {
        placement::place_investigator(self.write_access(), investigator, location);
    }

    /// Move `investigator` to `destination` by a card effect: a move, not a
    /// placement, so it fires the same leave and enter timing as the Move
    /// action — the left location's *"when … leaves"* and the entered one's
    /// *"after you enter"* abilities, the reveal, and engagement on entering.
    /// The destination need not connect; the native has already chosen it by
    /// its own card's rule. No attack of opportunity: that is the Move
    /// action's, not the move's.
    ///
    /// `Rejected`, changing nothing, when the investigator is not active at a
    /// location, `destination` is where they already are or not in play, or a
    /// rule forbids moving there
    /// ([`investigator_can_enter_location`](crate::engine::investigator_can_enter_location)).
    ///
    /// **Return its outcome.** The move runs on frames above everything the
    /// native leaves beneath it, so work written after this call would run
    /// before the move lands (ADR 0003).
    #[must_use = "move_investigator only queues the move: return its outcome (ADR 0003)"]
    pub fn move_investigator(
        &mut self,
        investigator: InvestigatorId,
        destination: LocationId,
    ) -> EngineOutcome {
        r#move::move_by_effect(self.write_access(), investigator, destination)
    }

    /// Apply the engagement rule: each ready, unengaged enemy at a location
    /// with an investigator engages one there (`glossary/Enemy_Engagement.md`:
    /// *"Any time a ready unengaged enemy is at the same location as an
    /// investigator, it engages that investigator, …"*). For an effect that leaves
    /// enemies ready and unengaged beside an investigator, once it has resolved.
    pub fn engage_ready_enemies(&mut self) {
        hunters::engage_ready_enemies(self.write_access());
    }

    // --- Tokens ---

    /// Place `count` doom on the current agenda. A no-op on an empty agenda
    /// deck. No event announces doom yet (#593).
    pub fn place_doom_on_agenda(&mut self, count: u8) {
        act_agenda::place_doom_on_agenda(self.write_access(), count);
    }

    /// Place `count` doom on the card instance `instance`, wherever it is on
    /// the board; a no-op once it has left play. The doom counts toward the
    /// agenda's threshold at the next check, not at once. No event announces
    /// doom yet (#593).
    pub fn place_doom_on_card(&mut self, instance: CardInstanceId, count: u8) {
        board::place_doom(self.write_access(), instance, count);
    }

    /// Remove up to `count` clues from the card instance `instance`, wherever
    /// it is on the board, saturating at zero. No event names clues leaving a
    /// card.
    pub fn remove_clues(&mut self, instance: CardInstanceId, count: u8) {
        board::remove_clues(self.write_access(), instance, count);
    }

    /// `investigator` spends `amount` uses of `kind` from `instance`, emitting
    /// [`Event::UsesSpent`](crate::event::Event::UsesSpent). If that empties a
    /// pool the card prints as discard-when-empty, the card is discarded to
    /// its owner's pile at once.
    ///
    /// # Errors
    ///
    /// [`SpendUsesError::NotInPlay`] when `instance` is not on the board;
    /// [`SpendUsesError::Undiscardable`] when the depletion discard has nowhere
    /// to take the card from.
    pub fn spend_uses(
        &mut self,
        investigator: InvestigatorId,
        instance: CardInstanceId,
        kind: UseKind,
        amount: u8,
    ) -> Result<(), SpendUsesError> {
        uses::spend_uses(self.write_access(), investigator, instance, kind, amount)
    }

    /// Move up to `amount` uses of `kind` off `instance` into
    /// `investigator`'s resource pool, each becoming a resource (Forbidden
    /// Knowledge 01058), emitting
    /// [`Event::UsesMovedToResourcePool`](crate::event::Event::UsesMovedToResourcePool)
    /// with the number moved. The move is neither a spend nor a gain, so
    /// [`Event::UsesSpent`](crate::event::Event::UsesSpent) and
    /// [`Event::ResourcesGained`](crate::event::Event::ResourcesGained) stay
    /// silent. If it empties a pool the card prints as discard-when-empty, the
    /// card is discarded to its owner's pile at once, as for
    /// [`spend_uses`](Self::spend_uses).
    ///
    /// # Errors
    ///
    /// [`MoveUsesError::NotInPlay`] when `instance` is not on the board;
    /// [`MoveUsesError::NoSuchInvestigator`] when `investigator` is not in the
    /// game; [`MoveUsesError::Undiscardable`] when the depletion discard has
    /// nowhere to take the card from.
    pub fn move_uses_to_resource_pool(
        &mut self,
        investigator: InvestigatorId,
        instance: CardInstanceId,
        kind: UseKind,
        amount: u8,
    ) -> Result<(), MoveUsesError> {
        uses::move_uses_to_resource_pool(self.write_access(), investigator, instance, kind, amount)
    }

    /// Move up to `amount` resources out of `investigator`'s resource pool onto
    /// the card instance `instance`, wherever it is on the board (Hospital
    /// Debts 01011), emitting
    /// [`Event::ResourcesMovedToCard`](crate::event::Event::ResourcesMovedToCard)
    /// with the number moved. A move, neither a payment nor a gain, so
    /// [`Event::ResourcesPaid`](crate::event::Event::ResourcesPaid) and
    /// [`Event::ResourcesGained`](crate::event::Event::ResourcesGained) stay
    /// silent. An empty pool moves nothing and announces nothing.
    ///
    /// # Errors
    ///
    /// [`MoveResourcesError::NotInPlay`] when `instance` is not on the board;
    /// [`MoveResourcesError::NoSuchInvestigator`] when `investigator` is not in
    /// the game.
    pub fn move_resources_to_card(
        &mut self,
        investigator: InvestigatorId,
        instance: CardInstanceId,
        amount: u8,
    ) -> Result<(), MoveResourcesError> {
        board::move_resources_to_card(self.write_access(), investigator, instance, amount)
    }

    /// Move up to `amount` horror from the card instance `from` onto the card
    /// instance `to`, wherever each is on the board (The Necronomicon 01009),
    /// emitting [`Event::HorrorMoved`](crate::event::Event::HorrorMoved) with
    /// the number moved. A move, neither a deal nor a heal, so
    /// [`Event::HorrorTaken`](crate::event::Event::HorrorTaken) stays silent
    /// and no harm window opens. A source with no horror moves nothing and
    /// announces nothing.
    ///
    /// A destination the horror defeats is defeated: an investigator card is
    /// eliminated, an asset discarded. So `to`'s card may be out of play when
    /// this returns.
    ///
    /// # Errors
    ///
    /// [`MoveHorrorError::NotInPlay`] when `from` or `to` is not on the board;
    /// [`MoveHorrorError::NoSanity`] when `to` has no sanity value.
    pub fn move_horror(
        &mut self,
        from: CardInstanceId,
        to: CardInstanceId,
        amount: u8,
    ) -> Result<(), MoveHorrorError> {
        combat::move_horror(self.write_access(), from, to, amount)
    }

    // --- Leaving play ---

    /// Discard `card` from play to its owner's pile, with its leave-play
    /// consequences. Returns where it was.
    ///
    /// # Errors
    ///
    /// [`NotLeft::NotInPlay`] if it was not in play, and [`NotLeft::Refused`]
    /// if it cannot leave play right now (The Necronomicon 01009 while it has
    /// horror); nothing changes either way.
    pub fn discard_from_play(
        &mut self,
        card: impl Into<LeavingCard>,
    ) -> Result<Placement, NotLeft> {
        board::discard_from_play(self.write_access(), card)
    }

    /// Remove `card` from the game, with its leave-play consequences. Returns
    /// where it was.
    ///
    /// # Errors
    ///
    /// [`NotLeft`], as for [`discard_from_play`](Self::discard_from_play).
    pub fn remove_from_game(&mut self, card: impl Into<LeavingCard>) -> Result<Placement, NotLeft> {
        board::remove_from_game(self.write_access(), card)
    }

    /// Remove the location `location` from the game, discarding what is
    /// attached to it or at it. Returns whether it was in play.
    pub fn remove_location_from_game(&mut self, location: LocationId) -> bool {
        board::remove_location_from_game(self.write_access(), location)
    }

    /// Discard `code`, an encounter card outside any zone of play, to the
    /// encounter discard, emitting
    /// [`Event::CardDiscarded`](crate::event::Event::CardDiscarded) naming
    /// `from`.
    pub fn discard_encounter_card(&mut self, code: CardCode, from: Zone) {
        board::discard_encounter_card(self.write_access(), code, from);
    }

    /// Discard a random card from `investigator`'s hand, returning it, or
    /// `None` for an empty hand.
    pub fn discard_random_from_hand(&mut self, investigator: InvestigatorId) -> Option<CardCode> {
        cards::discard_random_from_hand(self.write_access(), investigator)
    }

    // --- Decks ---

    /// Move the card `from` cards below the top of `deck` to `to` cards below
    /// the top, returning whether anything moved (an index past the bottom is
    /// a no-op). No event announces it: a deck's order is hidden.
    pub fn move_card_in_deck(&mut self, deck: Deck, from: usize, to: usize) -> bool {
        cards::move_card_in_deck(self.write_access(), deck, from, to)
    }

    // --- The encounter deck ---

    /// Take the top card of the encounter deck, or `None` if it is empty. Never
    /// reshuffles the discard in. The card is the caller's to put somewhere,
    /// as [`discard_encounter_card`](Self::discard_encounter_card) does.
    pub fn take_encounter_top(&mut self) -> Option<CardCode> {
        encounter::take_encounter_top(self.write_access())
    }

    /// Draw the top card of the encounter deck, shuffling the encounter
    /// discard back in first if the deck is empty — the encounter draw's own
    /// rule. `None` when the deck and the discard are both empty, a state no
    /// rule defines. The card is the caller's to resolve, as
    /// [`resolve_encounter_card`](Self::resolve_encounter_card) does.
    pub fn draw_encounter_top(&mut self) -> Option<CardCode> {
        encounter::draw_encounter_top(self.write_access())
    }

    /// Shuffle the encounter discard back into the encounter deck.
    pub fn reshuffle_encounter_discard(&mut self) {
        encounter::reshuffle_encounter_discard(self.write_access());
    }

    /// Resolve the already-drawn encounter card `code` for `investigator`: it
    /// is revealed, and the draw — whose resolve step is the Revelation, which
    /// a `when` ability may cancel — and the card's disposal (discard or spawn)
    /// are pushed as frames for the drive loop to step. `Rejected` for a
    /// cardtype that cannot be drawn this way.
    pub fn resolve_encounter_card(
        &mut self,
        investigator: InvestigatorId,
        code: CardCode,
        metadata: &CardMetadata,
    ) -> EngineOutcome {
        encounter::resolve_encounter_card(self.write_access(), investigator, code, metadata)
    }

    // --- Actions ---

    /// `investigator` loses `count` actions: they have that many fewer to take
    /// this turn, never fewer than none. Emits
    /// [`Event::ActionsRemainingChanged`](crate::event::Event::ActionsRemainingChanged)
    /// when the count changes.
    pub fn lose_actions(&mut self, investigator: InvestigatorId, count: u8) {
        take::lose_actions(self.write_access(), investigator, count);
    }

    // --- Acts and skill tests ---

    /// Advance the current act at round end, spending the clues its
    /// threshold needs from investigators at the location coded
    /// `contributor_location_code`. `Rejected` when they can't afford it.
    pub fn round_end_advance(&mut self, contributor_location_code: &str) -> EngineOutcome {
        act_agenda::round_end_advance(self.write_access(), contributor_location_code)
    }

    /// Register `substitution` for the rest of the round. No event announces
    /// it.
    pub fn substitute_skill_until_end_of_round(&mut self, substitution: SkillSubstitution) {
        skill_test::substitute_skill_until_end_of_round(self.write_access(), substitution);
    }

    /// Register `restriction` protecting `investigator` for the rest of the
    /// round (On the Lam 01010's [`Restriction::CannotAttack`]). No event
    /// announces it.
    pub fn restrict_until_end_of_round(
        &mut self,
        investigator: InvestigatorId,
        restriction: Restriction,
    ) {
        combat::restrict_until_end_of_round(self.write_access(), investigator, restriction);
    }

    /// Return the committed card at `index` — the one an
    /// `OnSkillTestResolution` effect is printed on, read from
    /// [`EvalContext::committed_card`](crate::engine::evaluator::EvalContext::committed_card)
    /// — to its investigator's hand when the test ends, instead of discarding
    /// it. Emits
    /// [`Event::CommittedCardReturnedToHand`](crate::event::Event::CommittedCardReturnedToHand)
    /// at RR ST.8, in place of the discard.
    pub fn return_committed_card_to_hand(&mut self, index: u8) {
        skill_test::return_committed_card_to_hand(self.write_access(), index);
    }
}
