#!/usr/bin/env bash
#
# ci-local.sh — run the CI jobs a diff can plausibly break, and skip the rest.
#
# CI (.github/workflows/ci.yml) runs seven jobs in parallel on every push; that
# is the guardrail. This script's job is narrower: catch, fast, the failures
# that are *predictable from the diff*.
#
# Each job below is invoked exactly as the workflow invokes it, with the same
# strict flags, so a job that runs here means what it means there. Two caveats,
# both reported at the end of a run rather than left implicit:
#
#   * `trunk` and `wasm-pack` are pinned in CI (trunk@0.21.14, wasm-pack@0.15.0)
#     but taken from $PATH here, so a local pass is only as good as your
#     installed versions.
#   * `classify` is not a CI job. It is a subset of `test`, offered on its own
#     for snapshot-only changes where the rest of `test` cannot be affected.
#
# The scoping rule is written against the reverse-dependency closure, not the
# touched paths. `web` sits downstream of `game-core`, `protocol`, and `cards`:
#
#     card-dsl -> game-core -> { cards, protocol }
#                              cards -> scenarios
#        web <- game-core, protocol, cards
#     server <- game-core, protocol, cards, scenarios
#
# so a `game-core` change can redden a wasm job even though it touched no file
# under `crates/web/`. Gating the wasm jobs on "did the diff touch crates/web?"
# would skip them on exactly the changes most likely to break them.
#
# Scoping is at job granularity only. Narrowing `cargo test --all` to
# `-p <crate>` is deliberately not done: a `game-core` change breaking `cards`'
# tests is the single most likely cross-crate breakage here, and per-crate
# tests are what would miss it.
#
# Usage:
#   scripts/ci-local.sh              # run the jobs this diff implicates
#   scripts/ci-local.sh --list       # print the plan, run nothing
#   scripts/ci-local.sh --all        # full seven-job gauntlet (escape hatch)
#   scripts/ci-local.sh --base <ref> # diff against <ref> instead of origin/main
#
set -uo pipefail

# `cd "$(git rev-parse ...)"` is not a guard: outside a repo the substitution is
# empty and `cd ""` succeeds. Check the ref parse itself.
ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "not inside a git repository" >&2
  exit 2
}
cd "$ROOT" || exit 2

# CI sets these workflow-wide, so every job below sees them.
export CARGO_TERM_COLOR=always
export RUSTFLAGS="-D warnings"

# The seven CI jobs, in the order the workflow lists them. Single source of
# truth: the --all plan and the end-of-run "not run locally" summary both read
# this, so adding an eighth job means touching this list and `run_job` only.
ALL_JOBS=(fmt clippy test doc wasm-build wasm-test wasm-clippy)

BASE=""
LIST_ONLY=0
FORCE_ALL=0

while [ $# -gt 0 ]; do
  case "$1" in
    --all)   FORCE_ALL=1; shift ;;
    --list)  LIST_ONLY=1; shift ;;
    --base)  BASE="${2:?--base needs a ref}"; shift 2 ;;
    -h|--help)
      sed -n '2,/^set -/p' "$0" | sed 's/^#\{0,1\} \{0,1\}//;$d'
      exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

# ---------------------------------------------------------------- changed files

# Prefer origin/main when it exists — a stale local main would under-report the
# diff, and under-reporting is the failure mode that lets a breakage through.
if [ -z "$BASE" ]; then
  if git rev-parse --verify --quiet origin/main >/dev/null; then
    BASE=origin/main
  else
    BASE=main
  fi
fi

MERGE_BASE=$(git merge-base "$BASE" HEAD 2>/dev/null) || {
  echo "cannot find a merge base with '$BASE'" >&2
  exit 2
}

# Committed-since-base, staged, unstaged, and untracked — everything that would
# land in the push but is not in $BASE.
CHANGED=$(
  {
    git diff --name-only "$MERGE_BASE"
    git ls-files --others --exclude-standard
  } | sort -u
)

if [ -z "$CHANGED" ] && [ "$FORCE_ALL" -eq 0 ]; then
  echo "no changes against $BASE ($MERGE_BASE) — nothing to check"
  exit 0
fi

diff_touches() { grep -qE "$1" <<<"$CHANGED"; }

# ----------------------------------------------------------------- job selection

declare -a PLAN=()
declare -A WHY=()
select_job() { PLAN+=("$1"); WHY["$1"]="$2"; }
planned() { [[ " ${PLAN[*]} " == *" $1 "* ]]; }

# A change to the CI definition, the cargo config, or the toolchain invalidates
# the mapping's assumptions wholesale — there is nothing to reason about, so
# run everything.
if [ "$FORCE_ALL" -eq 0 ] &&
   diff_touches '^\.github/workflows/|^\.cargo/|(^|/)rust-toolchain\.toml$'; then
  echo "CI config, cargo config, or toolchain changed — running the full gauntlet."
  FORCE_ALL=1
fi

if [ "$FORCE_ALL" -eq 1 ]; then
  for j in "${ALL_JOBS[@]}"; do select_job "$j" "--all"; done
else
  # fmt is ~1s. Gating it would cost more thought than running it.
  select_job fmt "always"

  if diff_touches '\.rs$|(^|/)Cargo\.(toml|lock)$'; then
    select_job clippy "rust sources or manifests changed"
    select_job test   "rust sources or manifests changed"
    # `doc` is deliberately NOT gated on "were doc comments added?". Intra-doc
    # links break most often when a public item is renamed, moved, or deleted —
    # the link dangles in a file the diff never touched, so there is no added
    # doc line anywhere to key off. A gate that cannot see its own failure class
    # is worse than no gate: it turns a caught break into a silent skip.
    select_job doc "rust sources or manifests changed"
  fi

  # web + protocol are what the wasm bundle is built from directly.
  if diff_touches '^crates/(web|protocol)/'; then
    select_job wasm-build  "crates/web or crates/protocol changed"
    select_job wasm-test   "crates/web or crates/protocol changed"
    select_job wasm-clippy "crates/web or crates/protocol changed"
  elif diff_touches '^crates/(game-core|cards|card-dsl)/'; then
    # Upstream of web, so a wasm build can still break — but the cheap job is
    # also the discerning one: wasm-clippy is the only job that compiles the
    # #[cfg(target_arch = "wasm32")] code the host `clippy` job never sees.
    select_job wasm-clippy "engine crates changed (upstream of web)"
  fi

  # The pipeline's `classify` tests are what catch a mis-vendored snapshot bump.
  # They already run under `test`; only add them when nothing else pulled it in.
  # Gated on `data/` alone, not on the docs-and-markdown case too: a change under
  # `docs/` cannot move a vendored pack file, so there is nothing for classify
  # to catch there.
  if diff_touches '^data/' && ! planned test; then
    select_job classify "data/ changed"
  fi
fi

echo "base:    $BASE ($(git rev-parse --short "$MERGE_BASE"))"
echo "changed: $(wc -l <<<"$CHANGED") file(s)"
echo "plan:"
for j in "${PLAN[@]}"; do printf '  %-12s %s\n' "$j" "${WHY[$j]}"; done
echo

if [ "$LIST_ONLY" -eq 1 ]; then
  exit 0
fi

# ------------------------------------------------------------------------- jobs

# Jobs that ran, but not the way CI runs them. Reported at the end so a
# degraded run is never mistaken for a clean one.
declare -a DEGRADED=()

run_job() {
  case "$1" in
    fmt)         cargo fmt --all -- --check ;;
    clippy)      cargo clippy --all-targets --all-features -- -D warnings ;;
    test)        cargo test --all --all-features ;;
    doc)         RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features ;;
    classify)    cargo test -p card-data-pipeline classify ;;
    wasm-clippy) cargo clippy -p web --all-targets --target wasm32-unknown-unknown --all-features -- -D warnings ;;
    wasm-test)   wasm-pack test --headless --firefox crates/web ;;
    wasm-build)
      # CI runs `trunk build --release` — the release profile and the asset
      # pipeline both fail in ways a debug cargo build does not. The fallback
      # is a strictly weaker check, so it is recorded as degraded rather than
      # allowed to report a clean pass.
      if command -v trunk >/dev/null; then
        (cd crates/web && trunk build --release)
      else
        echo "!! trunk not installed — falling back to a debug cargo build." >&2
        echo "!! This is weaker than CI's \`trunk build --release\`." >&2
        DEGRADED+=("wasm-build (no trunk: debug cargo build, not trunk --release)")
        cargo build -p web --target wasm32-unknown-unknown
      fi ;;
    *) echo "no such job: $1" >&2; return 2 ;;
  esac
}

declare -a FAILED=()
for j in "${PLAN[@]}"; do
  echo "==> $j"
  start=$(date +%s)
  if run_job "$j"; then
    echo "    ok ($(( $(date +%s) - start ))s)"
  else
    echo "    FAILED ($(( $(date +%s) - start ))s)"
    FAILED+=("$j")
  fi
done

echo
if [ ${#FAILED[@]} -gt 0 ]; then
  echo "failed: ${FAILED[*]}"
  exit 1
fi

SKIPPED=""
for j in "${ALL_JOBS[@]}"; do
  planned "$j" || SKIPPED+=" $j"
done

if [ -n "$SKIPPED" ]; then
  echo "passed. not run locally (CI will):${SKIPPED}"
else
  echo "passed. full gauntlet."
fi
if [ ${#DEGRADED[@]} -gt 0 ]; then
  printf 'ran degraded: %s\n' "${DEGRADED[@]}"
fi
