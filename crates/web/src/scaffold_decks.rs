//! The picker's placeholder decks, one per investigator it offers, until
//! decklist import (#782) retires both. Host-compiled, unlike the wasm-only
//! `picker` that seats them, so a host test can play a scenario with
//! the exact deck the picker sends (`tests/agnes_scaffold_walk.rs`,
//! `tests/daisy_scaffold_walk.rs`, `tests/skids_scaffold_walk.rs`).

/// Placeholder default deck for Roland (01001) until Phase 9 decklist import.
/// Implemented Guardian/Seeker/neutral cards only, so the opening hand is
/// playable. NOT a legal 30+1 deck — a scaffold for UI testing.
pub const ROLAND_DEFAULT_DECK: &[&str] = &[
    "01006", // .38 Special (signature)
    "01020", // Machete
    "01018", // Beat Cop
    "01021", // Guard Dog
    "01019", // First Aid
    "01024", // Dynamite Blast
    "01022", // Evidence!
    "01023", // Dodge
    "01025", // Vicious Blow
    "01030", // Magnifying Glass
    "01039", // Deduction
    "01037", // Working a Hunch
    "01089", // Guts
    "01090", // Perception
    "01091", // Overpower
    "01092", // Manual Dexterity
    "01093", // Unexpected Courage
    "01007", // Cover Up (signature weakness)
];

/// Placeholder default deck for Daisy Walker (01002) until decklist import
/// (#782) retires the scaffolds. Implemented cards she may take only, so every
/// card in it is playable: her two signatures at one copy each (Daisy's Tote
/// Bag 01008 and The Necronomicon 01009 weakness), then two of each Seeker
/// level-0 card and of the neutrals. NOT a legal deck — a scaffold for UI
/// testing, like [`ROLAND_DEFAULT_DECK`].
pub const DAISY_DEFAULT_DECK: &[&str] = &[
    "01008", // Daisy's Tote Bag (signature)
    "01030", "01030", // Magnifying Glass
    "01031", "01031", // Old Book of Lore
    "01032", "01032", // Research Librarian
    "01033", "01033", // Dr. Milan Christopher
    "01034", "01034", // Hyperawareness
    "01035", "01035", // Medical Texts
    "01036", "01036", // Mind over Matter
    "01037", "01037", // Working a Hunch
    "01038", "01038", // Barricade
    "01039", "01039", // Deduction
    "01086", "01086", // Knife
    "01087", "01087", // Flashlight
    "01088", "01088", // Emergency Cache
    "01089", "01089", // Guts
    "01090", "01090", // Perception
    "01091", "01091", // Overpower
    "01092", "01092", // Manual Dexterity
    "01093", "01093", // Unexpected Courage
    "01009", // The Necronomicon (signature weakness)
];

/// Placeholder default deck for Agnes Baker (01004) until decklist import
/// (#782) retires both scaffolds. Implemented cards she may take only, so every
/// card in it is playable: her two signatures at one copy each (Heirloom of
/// Hyperborea and the Dark Memory weakness, as her deckbuilding requirements
/// name them), two of each Mystic level-0 card and of the neutrals. NOT a legal
/// deck — a scaffold for UI testing, like [`ROLAND_DEFAULT_DECK`].
pub const AGNES_DEFAULT_DECK: &[&str] = &[
    "01012", // Heirloom of Hyperborea (signature)
    "01058", "01058", // Forbidden Knowledge
    "01059", "01059", // Holy Rosary
    "01060", "01060", // Shrivelling
    "01061", "01061", // Scrying
    "01062", "01062", // Arcane Studies
    "01063", "01063", // Arcane Initiate
    "01064", "01064", // Drawn to the Flame
    "01065", "01065", // Ward of Protection
    "01066", "01066", // Blinding Light
    "01067", "01067", // Fearless
    "01086", "01086", // Knife
    "01087", "01087", // Flashlight
    "01088", "01088", // Emergency Cache
    "01089", "01089", // Guts
    "01090", "01090", // Perception
    "01091", "01091", // Overpower
    "01092", "01092", // Manual Dexterity
    "01093", "01093", // Unexpected Courage
    "01013", // Dark Memory (signature weakness)
];

/// Placeholder default deck for "Skids" O'Toole (01003) until decklist import
/// (#782) retires the scaffolds. Implemented cards he may take only, so every
/// card in it is playable: two of each Guardian level-0 card (his
/// *"Guardian cards (\[guardian\]) level 0-2"*) and of the neutrals, two of
/// each Rogue level-0 card, and his two signatures at one copy each (On the
/// Lam and the Hospital Debts weakness, as his deckbuilding requirements name
/// them). NOT a legal deck — a scaffold for UI testing, like
/// [`ROLAND_DEFAULT_DECK`].
pub const SKIDS_DEFAULT_DECK: &[&str] = &[
    "01010", // On the Lam (signature)
    "01044", "01044", // Switchblade
    "01045", "01045", // Burglary
    "01046", "01046", // Pickpocketing
    "01047", "01047", // .41 Derringer
    "01048", "01048", // Leo De Luca
    "01049", "01049", // Hard Knocks
    "01050", "01050", // Elusive
    "01051", "01051", // Backstab
    "01052", "01052", // Sneak Attack
    "01053", "01053", // Opportunist
    "01016", "01016", // .45 Automatic
    "01017", "01017", // Physical Training
    "01018", "01018", // Beat Cop
    "01019", "01019", // First Aid
    "01020", "01020", // Machete
    "01021", "01021", // Guard Dog
    "01022", "01022", // Evidence!
    "01023", "01023", // Dodge
    "01024", "01024", // Dynamite Blast
    "01025", "01025", // Vicious Blow
    "01086", "01086", // Knife
    "01087", "01087", // Flashlight
    "01088", "01088", // Emergency Cache
    "01089", "01089", // Guts
    "01090", "01090", // Perception
    "01091", "01091", // Overpower
    "01092", "01092", // Manual Dexterity
    "01093", "01093", // Unexpected Courage
    "01011", // Hospital Debts (signature weakness)
];
