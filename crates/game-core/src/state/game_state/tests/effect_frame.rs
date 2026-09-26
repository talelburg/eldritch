use card_dsl::dsl::Effect;

use crate::engine::evaluator::EvalContext;
use crate::state::{Continuation, EffectFrame, InvestigatorId};

#[test]
fn effect_frame_variant_roundtrips_serde() {
    let frame = Continuation::Effect(EffectFrame::Seq {
        effects: vec![Effect::Seq(vec![])],
        next: 0,
        ctx: EvalContext::for_controller(InvestigatorId(1)),
    });
    let json = serde_json::to_string(&frame).expect("serialize");
    let back: Continuation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(frame, back);
}
