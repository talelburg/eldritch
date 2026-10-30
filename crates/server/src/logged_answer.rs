//! The **logged answer**: what an action-log entry records about the prompt it
//! answered and the options it picked, and the replay check that finds them
//! again (ADR 0022).
//!
//! Pure and I/O-free. [`record`] turns an accepted response into a
//! [`LoggedAnswer`], and [`replay`] turns one back into the response that
//! answers the prompt a re-run engine now asks, or says why it can't.
//! [`answered_options`] is the one lookup from a response to the options it
//! picked.

use std::fmt;

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{ChoiceOption, InputKind, InputRequest, OptionId, OptionTarget, Responder};
use game_core::state::investigator::InvestigatorId;
use serde::{Deserialize, Serialize};

/// One logged answer: the response, with each picked option named by what it
/// is rather than only by its position, and the identity of the prompt it
/// answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedAnswer {
    /// What the player answered.
    pub response: LoggedResponse,
    /// The prompt it answered.
    pub prompt: PromptIdentity,
}

/// A response as logged: an [`InputResponse`] whose picks carry the options
/// they named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoggedResponse {
    /// [`InputResponse::Confirm`].
    Confirm,
    /// [`InputResponse::Skip`].
    Skip,
    /// [`InputResponse::PickSingle`], with the option it picked.
    PickSingle {
        /// The picked option.
        option: LoggedOption,
    },
    /// [`InputResponse::PickMultiple`], with the options it selected, in the
    /// order the response named them.
    PickMultiple {
        /// The selected options.
        options: Vec<LoggedOption>,
    },
}

/// A picked option. Replay finds it by its key, `(label, target,
/// investigator)`; `id` is where it sat when it was picked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedOption {
    /// The option's id when it was picked.
    pub id: OptionId,
    /// [`ChoiceOption::label`].
    pub label: String,
    /// [`ChoiceOption::target`].
    pub target: Option<OptionTarget>,
    /// [`ChoiceOption::investigator`]. Part of the key because a window
    /// several answer can offer two investigators the same label on the same
    /// anchor (ADR 0019).
    pub investigator: Option<InvestigatorId>,
}

impl LoggedOption {
    fn of(option: &ChoiceOption) -> Self {
        Self {
            id: option.id,
            label: option.label.clone(),
            target: option.target.clone(),
            investigator: option.investigator,
        }
    }

    fn is(&self, option: &ChoiceOption) -> bool {
        self.label == option.label
            && self.target == option.target
            && self.investigator == option.investigator
    }
}

/// What identifies a prompt across builds: its kind, its anchor and who
/// answers it. Its text is left out, so a copy-edit doesn't strand a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptIdentity {
    /// [`InputRequest::kind`].
    pub kind: InputKind,
    /// [`InputRequest::target`].
    pub target: Option<OptionTarget>,
    /// [`InputRequest::responder`].
    pub responder: Responder,
}

impl PromptIdentity {
    /// The identity of `request`.
    #[must_use]
    pub fn of(request: &InputRequest) -> Self {
        Self {
            kind: request.kind,
            target: request.target.clone(),
            responder: request.responder,
        }
    }
}

/// An action this build can't log as the answer to the prompt the game awaits.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Unrecordable {
    /// It is a player action that answers no prompt.
    #[error("{0:?} answers no prompt")]
    NotAnAnswer(PlayerAction),
    /// The game awaits no answer.
    #[error("the game awaits no answer")]
    NoPrompt,
    /// It picks an id the prompt doesn't offer.
    #[error("option {} is not offered", .0.0)]
    NotOffered(OptionId),
    /// It is a response variant the log has no form for.
    #[error("the log has no form for {0:?}")]
    UnknownResponse(InputResponse),
}

/// The options `response` picks from `request`, in the order it names them:
/// none for a `Confirm` or `Skip`.
///
/// # Errors
///
/// [`Unrecordable`] if the response names an id `request` doesn't offer, or is
/// a variant the log has no form for.
pub fn answered_options<'a>(
    request: &'a InputRequest,
    response: &InputResponse,
) -> Result<Vec<&'a ChoiceOption>, Unrecordable> {
    let ids: &[OptionId] = match response {
        InputResponse::Confirm | InputResponse::Skip => &[],
        InputResponse::PickSingle(id) => std::slice::from_ref(id),
        InputResponse::PickMultiple { selected } => selected,
        other => return Err(Unrecordable::UnknownResponse(other.clone())),
    };
    ids.iter()
        .map(|id| {
            request
                .options
                .iter()
                .find(|option| option.id == *id)
                .ok_or(Unrecordable::NotOffered(*id))
        })
        .collect()
}

/// Log `response` as the answer to `request`.
///
/// # Errors
///
/// As [`answered_options`].
pub fn record(
    request: &InputRequest,
    response: &InputResponse,
) -> Result<LoggedAnswer, Unrecordable> {
    let mut picked = answered_options(request, response)?
        .into_iter()
        .map(LoggedOption::of);
    let response = match response {
        InputResponse::Confirm => LoggedResponse::Confirm,
        InputResponse::Skip => LoggedResponse::Skip,
        InputResponse::PickSingle(id) => LoggedResponse::PickSingle {
            option: picked.next().ok_or(Unrecordable::NotOffered(*id))?,
        },
        // `answered_options` has refused every other variant.
        _ => LoggedResponse::PickMultiple {
            options: picked.collect(),
        },
    };
    Ok(LoggedAnswer {
        response,
        prompt: PromptIdentity::of(request),
    })
}

/// Why a logged answer no longer replays. Each variant is one of the replay
/// checks ADR 0022 lists, in the order they run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReplayFailure {
    /// The engine now asks a different prompt, or none.
    PromptChanged {
        /// The prompt the answer was logged against.
        recorded: PromptIdentity,
        /// The prompt the engine asks now, if any.
        now: Option<PromptIdentity>,
    },
    /// A logged pick matches none of the prompt's options, or several.
    OptionNotFound {
        /// The logged option.
        option: LoggedOption,
        /// How many of the prompt's options it matches.
        matches: usize,
    },
    /// The engine rejects the answer.
    Rejected(String),
    /// The action drew a different number of times from the RNG.
    Draws {
        /// `RngState::draws` after the action applied, as logged.
        recorded: u64,
        /// `RngState::draws` after it applied on replay.
        now: u64,
    },
}

impl fmt::Display for ReplayFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PromptChanged { recorded, now } => {
                write!(
                    f,
                    "it answered {recorded:?}, and the engine now asks {now:?}"
                )
            }
            Self::OptionNotFound { option, matches } => write!(
                f,
                "its pick of \"{}\" (target {:?}, investigator {:?}) matches {matches} options",
                option.label, option.target, option.investigator,
            ),
            Self::Rejected(reason) => write!(f, "the engine rejects it: {reason}"),
            Self::Draws { recorded, now } => write!(
                f,
                "the RNG stood at {now} draws after it, where {recorded} were logged"
            ),
        }
    }
}

/// The response that gives `answer` to `request`, the prompt the engine asks
/// now.
///
/// Checks the prompt is the one `answer` was logged against, then finds each
/// logged pick among `request`'s options and answers with the id it has now.
///
/// # Errors
///
/// [`ReplayFailure::PromptChanged`] if `request`'s identity differs from the
/// logged one, or [`ReplayFailure::OptionNotFound`] if a logged pick matches
/// no option or several.
pub fn replay(
    request: &InputRequest,
    answer: &LoggedAnswer,
) -> Result<InputResponse, ReplayFailure> {
    let now = PromptIdentity::of(request);
    if now != answer.prompt {
        return Err(ReplayFailure::PromptChanged {
            recorded: answer.prompt.clone(),
            now: Some(now),
        });
    }
    Ok(match &answer.response {
        LoggedResponse::Confirm => InputResponse::Confirm,
        LoggedResponse::Skip => InputResponse::Skip,
        LoggedResponse::PickSingle { option } => {
            InputResponse::PickSingle(find(&request.options, option)?)
        }
        LoggedResponse::PickMultiple { options } => InputResponse::PickMultiple {
            selected: options
                .iter()
                .map(|option| find(&request.options, option))
                .collect::<Result<_, _>>()?,
        },
    })
}

/// The id `logged` has among `options`, found by its key.
fn find(options: &[ChoiceOption], logged: &LoggedOption) -> Result<OptionId, ReplayFailure> {
    let mut matching = options.iter().filter(|option| logged.is(option));
    match (matching.next(), matching.count()) {
        (Some(option), 0) => Ok(option.id),
        (first, rest) => Err(ReplayFailure::OptionNotFound {
            option: logged.clone(),
            matches: usize::from(first.is_some()) + rest,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_finds_the_option_offered_to_its_investigator() {
        let request = InputRequest::pick_single(
            "Trigger a reaction?",
            vec![
                ChoiceOption::new(OptionId(0), "Trigger")
                    .at(OptionTarget::Act)
                    .offered_to(InvestigatorId(1)),
                ChoiceOption::new(OptionId(1), "Trigger")
                    .at(OptionTarget::Act)
                    .offered_to(InvestigatorId(2)),
            ],
        )
        .answered_by(Responder::Several);
        // Logged when investigator 2's option sat first.
        let answer = LoggedAnswer {
            response: LoggedResponse::PickSingle {
                option: LoggedOption {
                    id: OptionId(0),
                    label: "Trigger".to_owned(),
                    target: Some(OptionTarget::Act),
                    investigator: Some(InvestigatorId(2)),
                },
            },
            prompt: PromptIdentity {
                kind: InputKind::PickSingle,
                target: None,
                responder: Responder::Several,
            },
        };

        assert_eq!(
            replay(&request, &answer),
            Ok(InputResponse::PickSingle(OptionId(1))),
        );
    }
}
