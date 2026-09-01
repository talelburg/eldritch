# A choice printed on one card is a decision, not a selection

[ADR 0011](0011-the-engine-names-the-surface-a-prompt-renders-on.md) settled **where** a control belongs: the client reads the anchor the engine attached, and never decides for itself. It also set a uniform rule for how an anchored option is offered — *"even a single option opens a context menu, so the player always sees and confirms what they commit to"* — with one stated exception, the four surfaces the engine names by role, exempted because *"a control whose label is 'End turn' has no such ambiguity, and wrapping it in a one-item menu adds a click to defend against a confusion that cannot occur."*

That exception has a second instance, and the menu rule was applied to it anyway.

An `Effect::ChooseOne` anchors to the card its effect is printed on, so the client offers its branches as a menu on that card — which the player must click to open. First Aid 01019 (`data/arkhamdb-snapshot/pack/core/core.json`):

> \[action\] Spend 1 supply: Heal 1 damage or horror from an investigator at your location.

You click the asset, you click **Activate**, the engine pays the supply — and then you must click the same asset again to choose which mode. The act and agenda are worse, because the advance's own acknowledge sits in front: act 3, **What Have You Done?** (01110), reverse:

> The lead investigator must decide (choose one):
> - It was never much of a home. Burn it down! **(→R1)**
> - This hell-pit is my home! No way are we burning it! **(→R2)**

The count is a symptom. The defect is that after committing to an ability the player's attention is on the card they committed to, and the card gives no signal that the game is now waiting on them — nothing distinguishes *this ability is resolving* from *this ability is available*, so the game reads as stalled.

**The menu rule's rationale does not reach these prompts.** It is about disambiguating among *entities*: a location or an enemy is one board entity carrying several possible actions, and the entity alone does not say which. A `ChooseOne` is not that. There is exactly one card, the engine is already mid-resolution, and the anchor records **where the text is printed** rather than **what is being selected**. So the extra click defends against a confusion that cannot occur here either.

**A prompt therefore declares its nature, and the client maps nature to presentation.**

```rust
#[non_exhaustive]
pub enum PromptNature {
    /// Options are board entities; the anchor disambiguates which one.
    #[default]
    Selection,
    /// Options are alternatives printed on one card; the anchor is provenance.
    Decision,
}
```

It rides on `InputRequest`, beside the anchor, rather than on each option: every
branch of one choice shares it. The anchor rides there too now, because the
surface presenting a decision reads *one* anchor to name where the text is
printed, while the options keep theirs — that is what makes the card glow.

A **selection** keeps its context menu. A **decision** presents itself the moment it arises, as a modal over a scrimmed board carrying the source card's name and printed text — the branch labels are already authored from that text, so the two agree by construction: 01019's two labels split its single *"damage or horror"* sentence, and 01110's are its two printed bullets, resolution markers included.

**The engine names the nature, never the presentation.** This is ADR 0011's claim one layer in, and the reason is the same: a `Presentation::Modal` on the wire would put UI vocabulary in the kernel, and the client re-deriving the nature from the prompt's shape — *all options anchored to the same card* — is exactly the string-matching ADR 0011 rejected when it refused to re-derive targets from `label`. The engine says what is being chosen between; the client decides what that looks like, and can change its mind without an engine release.

**The modal is the sole surface for a decision.** The card keeps its actionable glow, which is the remaining job of an anchor that no longer chooses placement, but opens no menu while the decision is live; and the prompt banner stands down the way it already does for the skill-test result modal's Confirm. Offering one choice on two surfaces is the double-render defect #541 fixed for anchored options.

**`Selection` is the default, and the default is the status quo.** Every existing constructor produces it; a builder opts in. So a future prompt whose author never considered its nature renders as today's context menu rather than as a modal in front of the map. That is the same knowingly-accepted shape ADR 0011 chose for un-anchored options and it carries the identical cost, stated there: *"a future builder that forgets `.at(…)` therefore compiles… That failure is caught by tests, not by the type."* Here, one test pins the decision at the `ChooseOne` site and one pins `Selection` at a representative selection site; deleting either re-opens the hole.

**Exactly one construction site is a decision.** The other thirteen `PickSingle` sites keep the default untouched. Making each declare its nature explicitly was rejected for the reason ADR 0011 already gives about asserting at sites that do not know the answer — it *"would be a silent wrong answer rather than a build error"*.

## Considered options

**Let the client infer it.** A `PickSingle` whose options are all anchored to one card, where that card is not among the entities being chosen between, is derivable without a protocol change. Rejected: it is intent re-derived from structure, and the derivation breaks the first time a genuine selection offers two abilities on one enemy.

**A third `InputKind` variant.** Rejected on the type level — a decision *is* a `PickSingle`. Kind says how you answer; nature says what you are answering. Collapsing them grows a case in every `match` on kind that does not care.

**A bool.** Reads as `if request.is_decision` at the call site and leaves no room for a third nature, which the excluded case below may yet force.

## Consequences

**Search-deck prompts are knowingly excluded.** Old Book of Lore 01031 — *"That investigator searches the top 3 cards of his or her deck for a card, draws it, and shuffles the remaining cards into his or her deck."* — and Research Librarian 01032 — *"Search your deck for a \[\[Tome\]\] asset and add it to your hand. Shuffle your deck."* — suspend for a pick whose options are cards **in a deck**: entities with no board surface at all, which is neither of this ADR's two natures. They have the same defect and land in the prompt banner. They are excluded because neither is reachable in the running client — the picker offers only the Roland scaffold deck, which contains neither — and **adding either card to that deck is a one-line change that inherits the gap**. Whether an off-board selection becomes a decision or earns a third nature is deferred to when one becomes reachable, not decided here.

**The two modals cannot collide.** The skill-test result modal's liveness predicate requires the live prompt to be the un-anchored acknowledge `Confirm`; a decision is a `PickSingle`. Mutually exclusive by construction, including for a `ChooseOne` nested under a skill test that has already resolved — the case that put an empty batch under the result panel in #853.

**The anchor stays, and stays liveness-checked.** The modal reads it to name its source card, so #845's degradation is still load-bearing: First Aid spending its *last* supply is discarded during cost payment, and an anchor pointing into the discard pile is a deadlock rather than a misplacement.

*ADR 0011 is not superseded: anchors, surfaces, and the menu rule for selections all stand, and this ADR is folded into its changelog as a refinement. Two edits landed there rather than being appended here, per `docs/agents/writing.md`: the menu rule now says which prompts it reaches, and a sentence naming First Aid 01019's activated ability as a dispatch site whose choices render in the banner is gone — it predated #834's collapse of the two-field `EvalContext`, and that ability has anchored to its own asset since.*
