//! The context a card-local native effect resolves in: read the whole state,
//! change the game only through the engine's exits.

use std::borrow::Cow;

use card_dsl::card_data::CardMetadata;
use card_dsl::dsl::Restriction;

use crate::card_registry::CardRegistry;
use crate::engine::board::{self, LeavingCard, MoveResourcesError, NotLeft, Placement};
use crate::engine::dispatch::actions::{r#move, take};
use crate::engine::dispatch::choice::{self, Suspension};
use crate::engine::dispatch::tokens::{self, MoveHorrorError};
use crate::engine::dispatch::{
    act_agenda, cards, combat, elimination, encounter, engagement, hunters, placement, reveal,
    set_aside, skill_test, threat_area,
    uses::{self, MoveUsesError, SpendUsesError},
};
use crate::engine::outcome::{EngineOutcome, ResumeToken};
use crate::engine::Cx;
use crate::event::TraumaKind;
use crate::state::{
    CardCode, CardInstanceId, Continuation, DamageSource, Deck, EffectPrompt, EnemyId, GameState,
    InvestigatorId, LocationId, Owner, SkillSubstitution, UseKind, Zone,
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
/// A native that suspends for a choice (returns what [`suspend`](Self::suspend)
/// returns) is re-run from the top when the choice is answered, so it must ask
/// before it writes. Every exit marks the context as written to when it is
/// called, even when it turns out to change nothing, and a native that suspends
/// after any exit call is `Rejected` in every build. The apply's snapshot
/// rollback then undoes what it wrote.
///
/// Read-only helpers, such as
/// [`resolve_grounded_choice`](crate::engine::resolve_grounded_choice),
/// [`resolve_card_choice`](crate::engine::resolve_card_choice) and
/// [`enemy_can_enter_location`](crate::engine::enemy_can_enter_location), are
/// free functions called on [`state`](Self::state), so asking never counts as
/// writing. No exit suspends by itself: one that needs a later step, such as an
/// encounter card's resolution or a spawned enemy's engagement tie, defers it
/// through a continuation frame and returns, and the frame's prompt surfaces
/// after the native.
///
/// # Nothing written past a pending prompt
///
/// So a native whose exit left a prompt pending must not then change the game
/// directly, or the change would land before the prompt is answered. Each exit
/// is classified as a direct write or a push; a direct-write exit called while
/// a prompt an earlier exit pushed is still pending makes the native
/// `Rejected`, and the apply rolls back. A push after it is still allowed, so
/// a native may fold several [`deal_harm`](Self::deal_harm)s. Exits that both
/// write and may push count as direct writes. Scaffolding: deleted when
/// natives defer their tail (#1184).
pub struct NativeCx<'a> {
    cx: Cx<'a>,
    written: bool,
    /// The stack depth when the native was entered: frames above it are the
    /// ones its exits pushed.
    entry_depth: usize,
    /// Set when a direct-write exit runs while a pushed prompt is pending.
    wrote_past_prompt: bool,
    /// The prompt the native suspended on through [`suspend`](Self::suspend),
    /// for the evaluator to store on the native's frame.
    asked: Option<EffectPrompt>,
}

/// How an exit changes the game, which decides whether it may run after an
/// earlier exit left a prompt pending.
///
/// An exit that pushes a frame returns before the frame runs, so a prompt it
/// raises (a deal of damage's soak, a spawned enemy's engagement tie) surfaces
/// only after the native returns. A **direct write** made after it would land
/// before that prompt is answered, out of printed order, so the native is
/// `Rejected`. A **push** made after it is fine: it resolves after the native
/// too, in the order the native pushed it — Dynamite Blast 01024's deals,
/// pushed in reverse so they resolve ascending.
///
/// Only a Prompt-profile frame counts as pending. A direct write after an exit
/// that queued an event emit (`EnemyDefeated` behind
/// [`deal_damage_to_enemy`](NativeCx::deal_damage_to_enemy)) is allowed, though
/// it lands before that event's reaction window opens.
///
/// Scaffolding: deleted when natives defer their tail (#1184).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitKind {
    /// Changes the game before returning, even if it then also pushes a frame
    /// (a mixed exit, such as
    /// [`put_set_aside_card_into_play`](NativeCx::put_set_aside_card_into_play),
    /// counts here).
    DirectWrite,
    /// Changes nothing before returning: it only pushes frames that do the
    /// work after the native.
    Push,
}

impl<'a> NativeCx<'a> {
    /// Wrap `cx` for one native call, not yet written to.
    pub(crate) fn new(cx: Cx<'a>) -> Self {
        let entry_depth = cx.state.continuations.len();
        Self {
            cx,
            written: false,
            entry_depth,
            wrote_past_prompt: false,
            asked: None,
        }
    }

    /// Whether any exit has been called since [`new`](Self::new).
    pub(crate) fn written(&self) -> bool {
        self.written
    }

    /// The prompt the native suspended on, if it called
    /// [`suspend`](Self::suspend).
    pub(crate) fn take_asked(&mut self) -> Option<EffectPrompt> {
        self.asked.take()
    }

    /// Suspend the native on what a grounding helper could not settle: the
    /// controller's pick, or the rejection of a pick out of range. **Return its
    /// outcome.**
    ///
    /// A pick suspends the native's effect frame in place, carrying the prompt,
    /// and the native is re-run with the pick threaded into its context
    /// ([`EvalContext::chosen_option`](crate::engine::evaluator::EvalContext::chosen_option))
    /// once it is answered.
    #[must_use = "suspend returns the outcome the native must return"]
    pub fn suspend(&mut self, suspension: Suspension) -> EngineOutcome {
        match suspension.into_prompt() {
            Ok(prompt) => {
                let request = choice::effect_request(&prompt);
                self.asked = Some(prompt);
                EngineOutcome::AwaitingInput {
                    request,
                    // TODO(#458): deterministic resume tokens.
                    resume_token: ResumeToken(0),
                }
            }
            Err(rejected) => rejected,
        }
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

    /// Whether a [direct-write](ExitKind::DirectWrite) exit was called while
    /// a prompt an earlier exit pushed was still pending.
    pub(crate) fn wrote_past_prompt(&self) -> bool {
        self.wrote_past_prompt
    }

    /// Mark the context written and hand out the inner `Cx` for one exit of
    /// `kind`. Every exit calls it, so every exit is classified.
    fn write_access(&mut self, kind: ExitKind) -> &mut Cx<'a> {
        self.written = true;
        if kind == ExitKind::DirectWrite && self.prompt_pending() {
            self.wrote_past_prompt = true;
        }
        &mut self.cx
    }

    /// Whether a frame an exit pushed during this call, above the stack depth
    /// the native was entered at, is a Prompt-profile frame.
    fn prompt_pending(&self) -> bool {
        self.cx
            .state
            .continuations
            .iter()
            .skip(self.entry_depth)
            .any(Continuation::awaits_input)
    }

    // --- Harm and defeat ---

    /// Deal `damage` + `horror` to `investigator`: the one way a native deals
    /// harm to an investigator. It is dealt the way an enemy attack's is — the
    /// player is offered the soak, and `DamageDealt` is announced with
    /// `DamagePlaced` nested in its resolve step — so nothing is placed when
    /// this returns.
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
        combat::deal_harm(
            self.write_access(ExitKind::Push),
            investigator,
            damage,
            horror,
            DamageSource::Effect,
            false,
        )
    }

    /// Defeat `investigator` by a card ability and run elimination. A no-op
    /// for an investigator who is not active.
    pub fn defeat_investigator(&mut self, investigator: InvestigatorId) {
        elimination::defeat_investigator(self.write_access(ExitKind::DirectWrite), investigator);
    }

    /// `investigator` suffers `amount` trauma of `kind`. Emits
    /// [`Event::TraumaSuffered`](crate::event::Event::TraumaSuffered); nothing
    /// records it yet. TODO(#766): record it in the campaign log.
    pub fn suffer_trauma(&mut self, investigator: InvestigatorId, kind: TraumaKind, amount: u8) {
        elimination::suffer_trauma(
            self.write_access(ExitKind::DirectWrite),
            investigator,
            kind,
            amount,
        );
    }

    /// Deal `amount` damage to `enemy`, defeating it if that is lethal. `by`
    /// names the investigator dealing it, if any.
    pub fn deal_damage_to_enemy(&mut self, enemy: EnemyId, amount: u8, by: Option<InvestigatorId>) {
        combat::deal_damage_to_enemy(self.write_access(ExitKind::DirectWrite), enemy, amount, by);
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
        set_aside::put_set_aside_card_into_play(
            self.write_access(ExitKind::DirectWrite),
            code,
            at,
            owner,
        )
    }

    /// Put the set-aside location `code` into play and return its minted
    /// id, wired to its in-play neighbours from the scenario's layout.
    ///
    /// # Errors
    ///
    /// The rejection reason, changing nothing, when `code` is not set aside,
    /// has no metadata, or is not a location.
    pub fn put_set_aside_location_into_play(
        &mut self,
        code: &str,
    ) -> Result<LocationId, Cow<'static, str>> {
        set_aside::put_set_aside_location_into_play(self.write_access(ExitKind::DirectWrite), code)
    }

    /// Reveal `location` if it is unrevealed, placing its printed clues.
    pub fn reveal_location(&mut self, location: LocationId) {
        reveal::reveal_location(self.write_access(ExitKind::DirectWrite), location);
    }

    /// Attach `code` to `location` under `owner`, returning its new instance
    /// id, or `None` when the location is not in play.
    pub fn attach_to_location(
        &mut self,
        location: LocationId,
        code: CardCode,
        owner: Owner,
    ) -> Option<CardInstanceId> {
        threat_area::attach_to_location(
            self.write_access(ExitKind::DirectWrite),
            location,
            code,
            owner,
        )
    }

    // --- Movement and placement ---

    /// Move `enemy` to `to` and, if it is unengaged, engage it there.
    pub fn relocate_enemy(&mut self, enemy: EnemyId, to: LocationId) {
        hunters::relocate_enemy(self.write_access(ExitKind::DirectWrite), enemy, to);
    }

    /// Place `investigator` at `location`. A placement, not a move: no move
    /// triggers fire. Emits
    /// [`Event::InvestigatorMoved`](crate::event::Event::InvestigatorMoved)
    /// only when the investigator was at a location and it differs from
    /// `location`.
    pub fn place_investigator(&mut self, investigator: InvestigatorId, location: LocationId) {
        placement::place_investigator(
            self.write_access(ExitKind::DirectWrite),
            investigator,
            location,
        );
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
        r#move::move_by_effect(self.write_access(ExitKind::Push), investigator, destination)
    }

    /// Apply the engagement rule: each ready, unengaged enemy at a location
    /// with an investigator engages one there (`glossary/Enemy_Engagement.md`:
    /// *"Any time a ready unengaged enemy is at the same location as an
    /// investigator, it engages that investigator, …"*). For an effect that leaves
    /// enemies ready and unengaged beside an investigator, once it has resolved.
    pub fn engage_ready_enemies(&mut self) {
        engagement::engage_ready_enemies(self.write_access(ExitKind::DirectWrite));
    }

    // --- Tokens ---

    /// Place `count` doom on the current agenda. A no-op on an empty agenda
    /// deck. No event announces doom yet. TODO(#593): announce it.
    pub fn place_doom_on_agenda(&mut self, count: u8) {
        act_agenda::place_doom_on_agenda(self.write_access(ExitKind::DirectWrite), count);
    }

    /// Place `count` doom on the card instance `instance`, wherever it is on
    /// the board; a no-op once it has left play. The doom counts toward the
    /// agenda's threshold at the next check, not at once. No event announces
    /// doom yet. TODO(#593): announce it.
    pub fn place_doom_on_card(&mut self, instance: CardInstanceId, count: u8) {
        board::place_doom(self.write_access(ExitKind::DirectWrite), instance, count);
    }

    /// Remove up to `count` clues from the card instance `instance`, wherever
    /// it is on the board, saturating at zero. No event names clues leaving a
    /// card.
    pub fn remove_clues(&mut self, instance: CardInstanceId, count: u8) {
        board::remove_clues(self.write_access(ExitKind::DirectWrite), instance, count);
    }

    /// `investigator` spends `amount` uses of `kind` from `instance`, emitting
    /// [`Event::UsesSpent`](crate::event::Event::UsesSpent). If that empties a
    /// pool the card prints as discard-when-empty, the card is discarded to
    /// its owner's pile at once.
    ///
    /// The discard is not deferred to the end of the ability. The
    /// discard-when-empty text is a constant ability, and *"Constant abilities
    /// have no point of initiation"* (Rules Reference, Ability), so it fires
    /// wherever uses change: a native's effect spending them here, as much as a
    /// `Cost::SpendUses` payment.
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
        uses::spend_uses(
            self.write_access(ExitKind::DirectWrite),
            investigator,
            instance,
            kind,
            amount,
        )
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
        uses::move_uses_to_resource_pool(
            self.write_access(ExitKind::DirectWrite),
            investigator,
            instance,
            kind,
            amount,
        )
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
        board::move_resources_to_card(
            self.write_access(ExitKind::DirectWrite),
            investigator,
            instance,
            amount,
        )
    }

    /// Discard every resource in `investigator`'s resource pool (Paranoia
    /// 01097), emitting
    /// [`Event::ResourcesDiscarded`](crate::event::Event::ResourcesDiscarded)
    /// with the number discarded. A discard, not a payment, so
    /// [`Event::ResourcesPaid`](crate::event::Event::ResourcesPaid) stays
    /// silent. An empty pool discards nothing and announces nothing.
    pub fn discard_all_resources(&mut self, investigator: InvestigatorId) {
        cards::discard_all_resources(self.write_access(ExitKind::DirectWrite), investigator);
    }

    /// Move up to `amount` horror from the card instance `from` onto the card
    /// instance `to`, wherever each is on the board (The Necronomicon 01009),
    /// emitting [`Event::HorrorMoved`](crate::event::Event::HorrorMoved) with
    /// the number moved. A move, neither a deal nor a heal, so
    /// [`Event::HorrorPlacedOnInvestigator`](crate::event::Event::HorrorPlacedOnInvestigator) stays silent
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
        tokens::move_horror(self.write_access(ExitKind::DirectWrite), from, to, amount)
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
        board::discard_from_play(self.write_access(ExitKind::DirectWrite), card)
    }

    /// Remove `card` from the game, with its leave-play consequences. Returns
    /// where it was.
    ///
    /// # Errors
    ///
    /// [`NotLeft`], as for [`discard_from_play`](Self::discard_from_play).
    pub fn remove_from_game(&mut self, card: impl Into<LeavingCard>) -> Result<Placement, NotLeft> {
        board::remove_from_game(self.write_access(ExitKind::DirectWrite), card)
    }

    /// Remove the location `location` from the game, discarding what is
    /// attached to it or at it. Returns whether it was in play.
    pub fn remove_location_from_game(&mut self, location: LocationId) -> bool {
        board::remove_location_from_game(self.write_access(ExitKind::DirectWrite), location)
    }

    /// Discard `code`, an encounter card outside any zone of play, to the
    /// encounter discard, emitting
    /// [`Event::CardDiscarded`](crate::event::Event::CardDiscarded) naming
    /// `from`.
    pub fn discard_encounter_card(&mut self, code: CardCode, from: Zone) {
        board::discard_drawn_card(
            self.write_access(ExitKind::DirectWrite),
            code,
            Owner::EncounterDeck,
            from,
        );
    }

    /// Discard a random card from `investigator`'s hand, returning it, or
    /// `None` for an empty hand.
    pub fn discard_random_from_hand(&mut self, investigator: InvestigatorId) -> Option<CardCode> {
        cards::discard_random_from_hand(self.write_access(ExitKind::DirectWrite), investigator)
    }

    /// Discard the card at `hand_index` of `investigator`'s hand to their
    /// discard pile. Nothing moves for an index past the hand's end or a
    /// missing investigator. Emits
    /// [`Event::CardDiscarded`](crate::event::Event::CardDiscarded) from
    /// [`Zone::Hand`].
    ///
    /// Each discard shifts the cards above it down one, so a native discarding
    /// several goes from the highest index to the lowest. Whether a weakness may
    /// be chosen is the caller's rule: `glossary/Weakness.md`, *"A player may
    /// not optionally choose to discard a weakness card from hand."*
    pub fn discard_from_hand(&mut self, investigator: InvestigatorId, hand_index: usize) {
        let in_hand = self
            .state()
            .investigators
            .get(&investigator)
            .is_some_and(|inv| hand_index < inv.hand.len());
        if in_hand {
            cards::discard_from_hand(
                self.write_access(ExitKind::DirectWrite),
                investigator,
                hand_index,
            );
        }
    }

    /// Remove every card in `investigator`'s discard pile from the game, into
    /// their removed-from-game pile, emitting one
    /// [`Event::DiscardPileRemovedFromGame`](crate::event::Event::DiscardPileRemovedFromGame).
    /// An empty pile changes nothing and emits nothing.
    pub fn remove_discard_pile_from_game(&mut self, investigator: InvestigatorId) {
        cards::remove_discard_pile_from_game(
            self.write_access(ExitKind::DirectWrite),
            investigator,
        );
    }

    /// Move the card at `index` of `investigator`'s discard pile to their
    /// hand, returning it, or `None` for an index past the pile's end. Emits
    /// [`Event::CardAddedToHandFromDiscard`](crate::event::Event::CardAddedToHandFromDiscard).
    pub fn add_discarded_card_to_hand(
        &mut self,
        investigator: InvestigatorId,
        index: usize,
    ) -> Option<CardCode> {
        cards::add_discarded_card_to_hand(
            self.write_access(ExitKind::DirectWrite),
            investigator,
            index,
        )
    }

    // --- Decks ---

    /// Move the card `from` cards below the top of `deck` to `to` cards below
    /// the top, returning whether anything moved (an index past the bottom is
    /// a no-op). No event announces it: a deck's order is hidden.
    pub fn move_card_in_deck(&mut self, deck: Deck, from: usize, to: usize) -> bool {
        cards::move_card_in_deck(self.write_access(ExitKind::DirectWrite), deck, from, to)
    }

    // --- The encounter deck ---

    /// Take the top card of the encounter deck, or `None` if it is empty. Never
    /// reshuffles the discard in. The card is the caller's to put somewhere,
    /// as [`discard_encounter_card`](Self::discard_encounter_card) does.
    pub fn take_encounter_top(&mut self) -> Option<CardCode> {
        encounter::take_encounter_top(self.write_access(ExitKind::DirectWrite))
    }

    /// Draw the top card of the encounter deck, shuffling the encounter
    /// discard back in first if the deck is empty — the encounter draw's own
    /// rule. `None` when the deck and the discard are both empty, a state no
    /// rule defines. The card is the caller's to resolve, as
    /// [`resolve_encounter_card`](Self::resolve_encounter_card) does.
    pub fn draw_encounter_top(&mut self) -> Option<CardCode> {
        encounter::draw_encounter_top(self.write_access(ExitKind::DirectWrite))
    }

    /// Shuffle the encounter discard back into the encounter deck.
    pub fn reshuffle_encounter_discard(&mut self) {
        encounter::reshuffle_encounter_discard(self.write_access(ExitKind::DirectWrite));
    }

    /// Resolve the already-drawn encounter card `code` for `investigator`: it
    /// is revealed, and the draw — whose resolve step is the Revelation, which
    /// a `when` ability may cancel — and the card's disposal (discard or spawn)
    /// are pushed as frames for the drive loop to step. `Rejected` for a
    /// cardtype that cannot be drawn this way.
    ///
    /// The card is the encounter deck's: both callers draw from it. A weakness
    /// drawn from a player deck takes the engine's own draw path instead.
    pub fn resolve_encounter_card(
        &mut self,
        investigator: InvestigatorId,
        code: CardCode,
        metadata: &CardMetadata,
    ) -> EngineOutcome {
        encounter::resolve_encounter_card(
            self.write_access(ExitKind::DirectWrite),
            investigator,
            code,
            metadata,
            Owner::EncounterDeck,
        )
    }

    // --- Actions ---

    /// `investigator` loses `count` actions: they have that many fewer to take
    /// this turn, never fewer than none. Emits
    /// [`Event::ActionsRemainingChanged`](crate::event::Event::ActionsRemainingChanged)
    /// when the count changes.
    pub fn lose_actions(&mut self, investigator: InvestigatorId, count: u8) {
        take::lose_actions(
            self.write_access(ExitKind::DirectWrite),
            investigator,
            count,
        );
    }

    // --- Acts and skill tests ---

    /// Advance the current act at round end, spending the clues its
    /// threshold needs from investigators at the location coded
    /// `contributor_location_code`. `Rejected` when they can't afford it.
    pub fn round_end_advance(&mut self, contributor_location_code: &str) -> EngineOutcome {
        act_agenda::round_end_advance(
            self.write_access(ExitKind::DirectWrite),
            contributor_location_code,
        )
    }

    /// Register `substitution` for the rest of the round. No event announces
    /// it.
    pub fn substitute_skill_until_end_of_round(&mut self, substitution: SkillSubstitution) {
        skill_test::substitute_skill_until_end_of_round(
            self.write_access(ExitKind::DirectWrite),
            substitution,
        );
    }

    /// Register `restriction` protecting `investigator` for the rest of the
    /// round (On the Lam 01010's [`Restriction::CannotAttack`]). No event
    /// announces it.
    pub fn restrict_until_end_of_round(
        &mut self,
        investigator: InvestigatorId,
        restriction: Restriction,
    ) {
        combat::restrict_until_end_of_round(
            self.write_access(ExitKind::DirectWrite),
            investigator,
            restriction,
        );
    }

    /// Return the committed card at `index` — the one an
    /// `OnSkillTestResolution` effect is printed on, read from
    /// [`EvalContext::committed_card`](crate::engine::evaluator::EvalContext::committed_card)
    /// — to its investigator's hand when the test ends, instead of discarding
    /// it. Emits
    /// [`Event::CommittedCardReturnedToHand`](crate::event::Event::CommittedCardReturnedToHand)
    /// at RR ST.8, in place of the discard.
    pub fn return_committed_card_to_hand(&mut self, index: u8) {
        skill_test::return_committed_card_to_hand(self.write_access(ExitKind::DirectWrite), index);
    }

    /// Discard `card` from play when the in-flight skill test ends (RR ST.8),
    /// after its results — an attack's damage included. Baseball Bat 01074's
    /// *"discard Baseball Bat after the attack resolves"*. Announced by the
    /// discard's own
    /// [`Event::CardDiscarded`](crate::event::Event::CardDiscarded) at ST.8.
    pub fn discard_after_this_test(&mut self, card: CardInstanceId) {
        skill_test::discard_after_this_test(self.write_access(ExitKind::DirectWrite), card);
    }
}
