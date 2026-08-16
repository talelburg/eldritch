#!/usr/bin/env bash
#
# ci-local.sh — run the CI jobs a diff can plausibly break, and skip the rest.
#
# CI (.github/workflows/ci.yml) runs seven jobs in parallel on every push; that
# is the guardrail. This script's job is narrower: catch, fast, the failures
# that are *predictable from the diff*. It runs each job with CI's exact
# invocation and strict flags, so a pass here means the same thing it means
# there.
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

cd "$(git rev-parse --show-toplevel)" || exit 1

# CI sets this workflow-wide, so every job below sees it.
export CARGO_TERM_COLOR=always
export RUSTFLAGS="-D warnings"

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

matches() { grep -qE "$1" <<<"$CHANGED"; }

# ----------------------------------------------------------------- job selection

declare -a PLAN=()
declare -A WHY=()
want() { PLAN+=("$1"); WHY["$1"]="$2"; }

if [ "$FORCE_ALL" -eq 1 ]; then
  want fmt         "--all"
  want clippy      "--all"
  want test        "--all"
  want doc         "--all"
  want wasm-build  "--all"
  want wasm-test   "--all"
  want wasm-clippy "--all"
else
  # fmt is ~1s. Gating it would cost more thought than running it.
  want fmt "always"

  if matches '\.rs$|(^|/)Cargo\.(toml|lock)$|(^|/)rust-toolchain\.toml$'; then
    want clippy "rust sources or manifests changed"
    want test   "rust sources or manifests changed"
  fi

  # `doc` can only fail on a doc comment or an intra-doc link, so gate it on
  # an *added* doc line rather than on .rs files generally.
  # Not `git diff … | grep -q`: grep exits on the first match, git dies of
  # SIGPIPE, and `pipefail` reports the whole pipeline as failed — so the
  # check silently never fires. Count instead; grep reads to EOF.
  DOC_LINES=$(git diff -U0 "$MERGE_BASE" -- '*.rs' | grep -cE '^\+\s*(///|//!|#!\[doc)')
  if [ "${DOC_LINES:-0}" -gt 0 ]; then
    want doc "doc comments added or changed"
  fi

  # web + protocol are what the wasm bundle is built from directly.
  if matches '^crates/(web|protocol)/'; then
    want wasm-build  "crates/web or crates/protocol changed"
    want wasm-test   "crates/web or crates/protocol changed"
    want wasm-clippy "crates/web or crates/protocol changed"
  elif matches '^crates/(game-core|cards|card-dsl)/'; then
    # Upstream of web, so a wasm build can still break — but the cheap job is
    # also the discerning one: wasm-clippy is the only job that compiles the
    # #[cfg(target_arch = "wasm32")] code the host `clippy` job never sees.
    want wasm-clippy "engine crates changed (upstream of web)"
  fi

  # The pipeline's `classify` tests are what catch a mis-vendored snapshot bump.
  # They already run under `test`; only add them when nothing else pulled it in.
  if matches '^data/' && ! [[ " ${PLAN[*]} " == *" test "* ]]; then
    want classify "data/ changed"
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
      # pipeline both fail in ways a debug cargo build does not. Fall back only
      # when trunk is missing, and say so, because the fallback is weaker.
      if command -v trunk >/dev/null; then
        (cd crates/web && trunk build --release)
      else
        echo "!! trunk not installed — falling back to a debug cargo build." >&2
        echo "!! This is weaker than CI's \`trunk build --release\`." >&2
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
for j in fmt clippy test doc wasm-build wasm-test wasm-clippy; do
  [[ " ${PLAN[*]} " == *" $j "* ]] || SKIPPED+=" $j"
done
if [ -n "$SKIPPED" ]; then
  echo "passed. not run locally (CI will):${SKIPPED}"
else
  echo "passed. full gauntlet."
fi
