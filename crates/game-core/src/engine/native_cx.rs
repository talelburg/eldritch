//! The context a card-local native effect resolves in: read the whole state,
//! change the game only through the engine's exits.

use std::borrow::Cow;
use std::cell::Cell;

use card_dsl::dsl::{Effect, Restriction};

use crate::card_registry::CardRegistry;
use crate::engine::board::{self, LeavingCard, MoveResourcesError, NotLeft, Placement};
use crate::engine::dispatch::choice::Suspension;
use crate::engine::dispatch::tokens::{self, MoveHorrorError};
use crate::engine::dispatch::{
    cards, encounter, set_aside, threat_area,
    uses::{self, MoveUsesError, SpendUsesError},
};
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::native_op::NativeOp;
use crate::engine::native_tail::StackMark;
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::TraumaKind;
use crate::state::{
    CardCode, CardInstanceId, Deck, EnemyId, GameState, InvestigatorId, LocationId,
    NativeTailFrame, Owner, SkillSubstitution, UseKind, Zone,
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
/// # A native resolves in the order it is written
///
/// Some exits leave work pending: [`deal_harm`](Self::deal_harm) pushes the
/// deal and returns before the harm lands, and a move, an encounter card's
/// resolution, a spawned enemy's engagement tie or an elimination does the
/// same. From the first such exit on, every exit the native calls is
/// **deferred**: held as an op on a tail frame beneath the pending work, and run
/// in call order once that work has resolved. A deferred exit returns `Done`,
/// meaning queued (ADR 0003); if its op is `Rejected` when it runs, the whole
/// apply is rejected and rolls back.
///
/// An exit that only queues the timing cells of a change it has already made
/// (an enemy defeat's `EnemyDefeated`) leaves nothing of the effect pending, so
/// the exits after it still run at once.
///
/// After a deferral the native can no longer see the board as it will be, so
/// a [`state`](Self::state) read, or an exit that returns a value
/// ([`take_encounter_top`](Self::take_encounter_top),
/// [`discard_from_play`](Self::discard_from_play), ...), makes the native
/// `Rejected`, and the apply rolls back. See
/// `docs/adr/0020-a-native-resolves-in-program-order.md`.
pub struct NativeCx<'a> {
    cx: Cx<'a>,
    written: bool,
    /// The stack as it stood when the native was entered, so the frames its
    /// exits push can be found.
    entry: StackMark,
    /// Set once an exit has left work pending: every exit after it is
    /// deferred.
    deferred: bool,
    /// The deferred exit calls, in call order.
    tail: Vec<NativeOp>,
    /// Set when the native read the game after the deferral began.
    read_after_deferral: Cell<bool>,
    /// The `Effect::Native` node being resolved and the context it resolves
    /// in: what [`suspend`](Self::suspend) re-pushes as the effect frame the
    /// native's prompt rests on.
    node: &'a Effect,
    eval_ctx: EvalContext,
    /// Set when the native suspended through [`suspend`](Self::suspend).
    suspended: bool,
}

impl<'a> NativeCx<'a> {
    /// Wrap `cx` for one call of the native `node` resolves to, in
    /// `eval_ctx`, not yet written to.
    pub(crate) fn new(cx: Cx<'a>, node: &'a Effect, eval_ctx: EvalContext) -> Self {
        let entry = StackMark::of(cx.state);
        Self {
            cx,
            written: false,
            entry,
            deferred: false,
            tail: Vec::new(),
            read_after_deferral: Cell::new(false),
            node,
            eval_ctx,
            suspended: false,
        }
    }

    /// Whether any exit has been called since [`new`](Self::new).
    pub(crate) fn written(&self) -> bool {
        self.written
    }

    /// Whether the native suspended through [`suspend`](Self::suspend).
    pub(crate) fn suspended(&self) -> bool {
        self.suspended
    }

    /// Whether the native read the game, through [`state`](Self::state) or an
    /// exit that returns a value, after an exit had left work pending.
    pub(crate) fn read_after_deferral(&self) -> bool {
        self.read_after_deferral.get()
    }

    /// Park the deferred exit calls, if any, as a tail frame directly beneath
    /// the frame that started the deferral. Called once the native has
    /// returned.
    pub(crate) fn park_tail(self) {
        if self.tail.is_empty() {
            return;
        }
        let Some(at) = self.entry.deferral_start(self.cx.state) else {
            unreachable!("a deferred exit was called with no deferral started");
        };
        let Effect::Native { tag } = self.node else {
            unreachable!("a NativeCx resolves an Effect::Native node");
        };
        self.cx.state.continuations.insert_beneath(
            at,
            NativeTailFrame {
                native: tag.clone(),
                ops: self.tail,
                next: 0,
            },
        );
    }

    /// Suspend the native on what a grounding helper could not settle: the
    /// controller's pick, or the rejection of a pick out of range. **Return its
    /// outcome.**
    ///
    /// A pick re-pushes the native's effect frame carrying the prompt, and
    /// suspends on it the one way the engine suspends (`prompt::suspend`), so
    /// the prompt table builds the request. The native is re-run with the pick
    /// threaded into its context
    /// ([`EvalContext::chosen_option`]) once it is answered.
    #[must_use = "suspend returns the outcome the native must return"]
    pub fn suspend(&mut self, suspension: Suspension) -> EngineOutcome {
        match suspension.into_prompt() {
            Ok(prompt) => {
                self.suspended = true;
                evaluator::suspend_leaf_in_place(&mut self.cx, self.node, self.eval_ctx, prompt)
            }
            Err(rejected) => rejected,
        }
    }

    /// The game state, read-only. A read after an exit left work pending makes
    /// the native `Rejected`: the board it shows is not yet the board the
    /// pending work will leave.
    #[must_use]
    pub fn state(&self) -> &GameState {
        if self.deferred {
            self.read_after_deferral.set(true);
        }
        self.cx.state
    }

    /// The card registry this apply runs against, for a read helper that
    /// looks a card up. Never refused: the registry doesn't change during an
    /// apply.
    #[must_use]
    pub fn cards(&self) -> &CardRegistry {
        self.cx.cards
    }

    /// Call an exit that returns a value: run `exit` on the inner `Cx` and
    /// note whether it left work pending. After a deferral the call is a read
    /// as much as a write, so it makes the native `Rejected`.
    fn value_exit<R>(&mut self, exit: impl FnOnce(&mut Cx<'a>) -> R) -> R {
        self.written = true;
        if self.deferred {
            self.read_after_deferral.set(true);
        }
        let value = exit(&mut self.cx);
        self.note_deferral();
        value
    }

    /// Run `op`, the call of an exit that returns `()` or an
    /// [`EngineOutcome`], or defer it if an earlier exit left work pending.
    /// What an op of a `()` exit returns is always `Done`, and so is a deferred
    /// op's: queued.
    fn run_op(&mut self, op: NativeOp) -> EngineOutcome {
        self.written = true;
        if self.deferred {
            self.tail.push(op);
            return EngineOutcome::Done;
        }
        let outcome = op.run(&mut self.cx);
        self.note_deferral();
        outcome
    }

    /// Start the deferral if an exit has pushed a frame that leaves work
    /// pending.
    fn note_deferral(&mut self) {
        if !self.deferred {
            self.deferred = self.entry.deferral_start(self.cx.state).is_some();
        }
    }

    // --- Harm and defeat ---

    /// Deal `damage` + `horror` to `investigator`: the one way a native deals
    /// harm to an investigator. It is dealt the way an enemy attack's is — the
    /// player is offered the soak, and `DamageDealt` is announced with
    /// `DamagePlaced` nested in its resolve step — so nothing is placed when
    /// this returns.
    ///
    /// The deal is left pending, so every exit called after this one waits
    /// until it has resolved (ADR 0020). **Propagate an outcome that is not
    /// `Done`**: a deal deferred behind earlier work returns `Done`, meaning
    /// queued (ADR 0003).
    #[must_use = "deal_harm may be Rejected: propagate an outcome that is not Done"]
    pub fn deal_harm(
        &mut self,
        investigator: InvestigatorId,
        damage: u8,
        horror: u8,
    ) -> EngineOutcome {
        self.run_op(NativeOp::DealHarm {
            investigator,
            damage,
            horror,
        })
    }

    /// Defeat `investigator` by a card ability and run elimination. A no-op
    /// for an investigator who is not active.
    pub fn defeat_investigator(&mut self, investigator: InvestigatorId) {
        self.run_op(NativeOp::DefeatInvestigator { investigator });
    }

    /// `investigator` suffers `amount` trauma of `kind`. Emits
    /// [`Event::TraumaSuffered`](crate::event::Event::TraumaSuffered); nothing
    /// records it yet. TODO(#766): record it in the campaign log.
    pub fn suffer_trauma(&mut self, investigator: InvestigatorId, kind: TraumaKind, amount: u8) {
        self.run_op(NativeOp::SufferTrauma {
            investigator,
            kind,
            amount,
        });
    }

    /// Deal `amount` damage to `enemy`, defeating it if that is lethal. `by`
    /// names the investigator dealing it, if any.
    pub fn deal_damage_to_enemy(&mut self, enemy: EnemyId, amount: u8, by: Option<InvestigatorId>) {
        self.run_op(NativeOp::DealDamageToEnemy { enemy, amount, by });
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
        self.run_op(NativeOp::PutSetAsideCardIntoPlay {
            code: CardCode::from(code),
            at: at.map(CardCode::from),
            owner,
        })
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
        self.value_exit(|cx| set_aside::put_set_aside_location_into_play(cx, code))
    }

    /// Reveal `location` if it is unrevealed, placing its printed clues.
    pub fn reveal_location(&mut self, location: LocationId) {
        self.run_op(NativeOp::RevealLocation { location });
    }

    /// Attach `code` to `location` under `owner`, returning its new instance
    /// id, or `None` when the location is not in play.
    pub fn attach_to_location(
        &mut self,
        location: LocationId,
        code: CardCode,
        owner: Owner,
    ) -> Option<CardInstanceId> {
        self.value_exit(|cx| threat_area::attach_to_location(cx, location, code, owner))
    }

    // --- Movement and placement ---

    /// Move `enemy` to `to` and, if it is unengaged, engage it there.
    pub fn relocate_enemy(&mut self, enemy: EnemyId, to: LocationId) {
        self.run_op(NativeOp::RelocateEnemy { enemy, to });
    }

    /// Place `investigator` at `location`. A placement, not a move: no move
    /// triggers fire. Emits
    /// [`Event::InvestigatorMoved`](crate::event::Event::InvestigatorMoved)
    /// only when the investigator was at a location and it differs from
    /// `location`.
    pub fn place_investigator(&mut self, investigator: InvestigatorId, location: LocationId) {
        self.run_op(NativeOp::PlaceInvestigator {
            investigator,
            location,
        });
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
    /// The move is left pending, so every exit called after this one waits
    /// until it has resolved (ADR 0020). **Propagate an outcome that is not
    /// `Done`**: `Done` means queued (ADR 0003).
    #[must_use = "move_investigator may be Rejected: propagate an outcome that is not Done"]
    pub fn move_investigator(
        &mut self,
        investigator: InvestigatorId,
        destination: LocationId,
    ) -> EngineOutcome {
        self.run_op(NativeOp::MoveInvestigator {
            investigator,
            destination,
        })
    }

    /// Apply the engagement rule: each ready, unengaged enemy at a location
    /// with an investigator engages one there (`glossary/Enemy_Engagement.md`:
    /// *"Any time a ready unengaged enemy is at the same location as an
    /// investigator, it engages that investigator, …"*). For an effect that leaves
    /// enemies ready and unengaged beside an investigator, once it has resolved.
    pub fn engage_ready_enemies(&mut self) {
        self.run_op(NativeOp::EngageReadyEnemies);
    }

    // --- Tokens ---

    /// Place `count` doom on the current agenda. A no-op on an empty agenda
    /// deck. No event announces doom yet. TODO(#593): announce it.
    pub fn place_doom_on_agenda(&mut self, count: u8) {
        self.run_op(NativeOp::PlaceDoomOnAgenda { count });
    }

    /// Place `count` doom on the card instance `instance`, wherever it is on
    /// the board; a no-op once it has left play. The doom counts toward the
    /// agenda's threshold at the next check, not at once. No event announces
    /// doom yet. TODO(#593): announce it.
    pub fn place_doom_on_card(&mut self, instance: CardInstanceId, count: u8) {
        self.run_op(NativeOp::PlaceDoomOnCard { instance, count });
    }

    /// Remove up to `count` clues from the card instance `instance`, wherever
    /// it is on the board, saturating at zero. No event names clues leaving a
    /// card.
    pub fn remove_clues(&mut self, instance: CardInstanceId, count: u8) {
        self.run_op(NativeOp::RemoveClues { instance, count });
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
        self.value_exit(|cx| uses::spend_uses(cx, investigator, instance, kind, amount))
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
        self.value_exit(|cx| {
            uses::move_uses_to_resource_pool(cx, investigator, instance, kind, amount)
        })
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
        self.value_exit(|cx| board::move_resources_to_card(cx, investigator, instance, amount))
    }

    /// Discard every resource in `investigator`'s resource pool (Paranoia
    /// 01097), emitting
    /// [`Event::ResourcesDiscarded`](crate::event::Event::ResourcesDiscarded)
    /// with the number discarded. A discard, not a payment, so
    /// [`Event::ResourcesPaid`](crate::event::Event::ResourcesPaid) stays
    /// silent. An empty pool discards nothing and announces nothing.
    pub fn discard_all_resources(&mut self, investigator: InvestigatorId) {
        self.run_op(NativeOp::DiscardAllResources { investigator });
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
        self.value_exit(|cx| tokens::move_horror(cx, from, to, amount))
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
        self.value_exit(|cx| board::discard_from_play(cx, card))
    }

    /// Remove `card` from the game, with its leave-play consequences. Returns
    /// where it was.
    ///
    /// # Errors
    ///
    /// [`NotLeft`], as for [`discard_from_play`](Self::discard_from_play).
    pub fn remove_from_game(&mut self, card: impl Into<LeavingCard>) -> Result<Placement, NotLeft> {
        self.value_exit(|cx| board::remove_from_game(cx, card))
    }

    /// Remove the location `location` from the game, discarding what is
    /// attached to it or at it. Returns whether it was in play.
    pub fn remove_location_from_game(&mut self, location: LocationId) -> bool {
        self.value_exit(|cx| board::remove_location_from_game(cx, location))
    }

    /// Discard `code`, an encounter card outside any zone of play, to the
    /// encounter discard, emitting
    /// [`Event::CardDiscarded`](crate::event::Event::CardDiscarded) naming
    /// `from`.
    pub fn discard_encounter_card(&mut self, code: CardCode, from: Zone) {
        self.run_op(NativeOp::DiscardEncounterCard { code, from });
    }

    /// Discard a random card from `investigator`'s hand, returning it, or
    /// `None` for an empty hand.
    pub fn discard_random_from_hand(&mut self, investigator: InvestigatorId) -> Option<CardCode> {
        self.value_exit(|cx| cards::discard_random_from_hand(cx, investigator))
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
        self.run_op(NativeOp::DiscardFromHand {
            investigator,
            hand_index,
        });
    }

    /// Remove every card in `investigator`'s discard pile from the game, into
    /// their removed-from-game pile, emitting one
    /// [`Event::DiscardPileRemovedFromGame`](crate::event::Event::DiscardPileRemovedFromGame).
    /// An empty pile changes nothing and emits nothing.
    pub fn remove_discard_pile_from_game(&mut self, investigator: InvestigatorId) {
        self.run_op(NativeOp::RemoveDiscardPileFromGame { investigator });
    }

    /// Move the card at `index` of `investigator`'s discard pile to their
    /// hand, returning it, or `None` for an index past the pile's end. Emits
    /// [`Event::CardAddedToHandFromDiscard`](crate::event::Event::CardAddedToHandFromDiscard).
    pub fn add_discarded_card_to_hand(
        &mut self,
        investigator: InvestigatorId,
        index: usize,
    ) -> Option<CardCode> {
        self.value_exit(|cx| cards::add_discarded_card_to_hand(cx, investigator, index))
    }

    // --- Decks ---

    /// Move the card `from` cards below the top of `deck` to `to` cards below
    /// the top, returning whether anything moved (an index past the bottom is
    /// a no-op). No event announces it: a deck's order is hidden.
    pub fn move_card_in_deck(&mut self, deck: Deck, from: usize, to: usize) -> bool {
        self.value_exit(|cx| cards::move_card_in_deck(cx, deck, from, to))
    }

    // --- The encounter deck ---

    /// Take the top card of the encounter deck, or `None` if it is empty. Never
    /// reshuffles the discard in. The card is the caller's to put somewhere,
    /// as [`discard_encounter_card`](Self::discard_encounter_card) does.
    pub fn take_encounter_top(&mut self) -> Option<CardCode> {
        self.value_exit(|cx| encounter::take_encounter_top(cx))
    }

    /// Draw the top card of the encounter deck, shuffling the encounter
    /// discard back in first if the deck is empty — the encounter draw's own
    /// rule. `None` when the deck and the discard are both empty, a state no
    /// rule defines. The card is the caller's to resolve, as
    /// [`resolve_encounter_card`](Self::resolve_encounter_card) does.
    pub fn draw_encounter_top(&mut self) -> Option<CardCode> {
        self.value_exit(|cx| encounter::draw_encounter_top(cx))
    }

    /// Shuffle the encounter discard back into the encounter deck.
    pub fn reshuffle_encounter_discard(&mut self) {
        self.run_op(NativeOp::ReshuffleEncounterDiscard);
    }

    /// Resolve the already-drawn encounter card `code` for `investigator`: it
    /// is revealed, and the draw — whose resolve step is the Revelation, which
    /// a `when` ability may cancel — and the card's disposal (discard or spawn)
    /// are pushed as frames for the drive loop to step. `Rejected` for a
    /// cardtype that cannot be drawn this way, or a code the card registry
    /// doesn't know.
    ///
    /// The card is the encounter deck's: both callers draw from it. A weakness
    /// drawn from a player deck takes the engine's own draw path instead.
    pub fn resolve_encounter_card(
        &mut self,
        investigator: InvestigatorId,
        code: CardCode,
    ) -> EngineOutcome {
        self.run_op(NativeOp::ResolveEncounterCard { investigator, code })
    }

    // --- Actions ---

    /// `investigator` loses `count` actions: they have that many fewer to take
    /// this turn, never fewer than none. Emits
    /// [`Event::ActionsRemainingChanged`](crate::event::Event::ActionsRemainingChanged)
    /// when the count changes.
    pub fn lose_actions(&mut self, investigator: InvestigatorId, count: u8) {
        self.run_op(NativeOp::LoseActions {
            investigator,
            count,
        });
    }

    // --- Acts and skill tests ---

    /// Advance the current act at round end, spending the clues its
    /// threshold needs from investigators at the location coded
    /// `contributor_location_code`. `Rejected` when they can't afford it.
    pub fn round_end_advance(&mut self, contributor_location_code: &str) -> EngineOutcome {
        self.run_op(NativeOp::RoundEndAdvance {
            contributor_location_code: CardCode::from(contributor_location_code),
        })
    }

    /// Register `substitution` for the rest of the round. No event announces
    /// it.
    pub fn substitute_skill_until_end_of_round(&mut self, substitution: SkillSubstitution) {
        self.run_op(NativeOp::SubstituteSkillUntilEndOfRound { substitution });
    }

    /// Register `restriction` protecting `investigator` for the rest of the
    /// round (On the Lam 01010's [`Restriction::CannotAttack`]). No event
    /// announces it.
    pub fn restrict_until_end_of_round(
        &mut self,
        investigator: InvestigatorId,
        restriction: Restriction,
    ) {
        self.run_op(NativeOp::RestrictUntilEndOfRound {
            investigator,
            restriction,
        });
    }

    /// Return the committed card at `index` — the one an
    /// `OnSkillTestResolution` effect is printed on, read from
    /// [`EvalContext::committed_card`](crate::engine::evaluator::EvalContext::committed_card)
    /// — to its investigator's hand when the test ends, instead of discarding
    /// it. Emits
    /// [`Event::CommittedCardReturnedToHand`](crate::event::Event::CommittedCardReturnedToHand)
    /// at RR ST.8, in place of the discard.
    pub fn return_committed_card_to_hand(&mut self, index: u8) {
        self.run_op(NativeOp::ReturnCommittedCardToHand { index });
    }

    /// Discard `card` from play when the in-flight skill test ends (RR ST.8),
    /// after its results — an attack's damage included. Baseball Bat 01074's
    /// *"discard Baseball Bat after the attack resolves"*. Announced by the
    /// discard's own
    /// [`Event::CardDiscarded`](crate::event::Event::CardDiscarded) at ST.8.
    pub fn discard_after_this_test(&mut self, card: CardInstanceId) {
        self.run_op(NativeOp::DiscardAfterThisTest { card });
    }
}
