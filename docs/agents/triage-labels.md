# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used in this repo's issue tracker.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the corresponding label string from this table.

## Current state on `talelburg/eldritch`

Labels are created on first use; `wontfix` and `ready-for-agent` are in use today.

These five are an **additive axis** — they do not replace the repo's existing labels. In particular the pre-existing `ready` label is coarser than `ready-for-agent` / `ready-for-human` and carries no claim about who implements the issue; see `docs/agents/issue-tracker.md` for the full existing vocabulary.
