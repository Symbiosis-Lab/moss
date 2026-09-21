#!/usr/bin/env bash
# Run the CSS cascade / theme render gates for published-page CSS.
#
# Some of them build a throwaway moss site with `target/debug/moss-cli`,
# serve it, and assert computed styles in BOTH chromium and webkit —
# assertions jsdom cannot make, because it does not implement @layer
# precedence. Most build no site at all: they either serve a vite harness or
# inject the branch's real stylesheets straight into `page.setContent`.
# GATES_BUILD and GATES_NOBUILD below are that split, by evidence (reads
# MOSS_BIN, calls buildScratchSite, or imports tests/e2e/helpers/scratch-site)
# rather than by name, so a gate wrongly filed in GATES_NOBUILD fails loudly
# there instead of passing having never opened the binary it needed.
#
# This is the public half of the desktop repo's own `scripts/render-gates.sh`:
# only the gates whose subject is a published page's CSS (site.css +
# cascade), ported per docs/archive/2026-09-20-ci-cost-cut-plan.md (C2a).
# App/editor/preview gates stay in the private desktop repo, because their
# subject is the desktop app's own chrome, not anything this repo builds.
#
# `moss-cli build` opens no window (ADR-050), so no display wrapper is needed
# even on Linux — unlike the desktop repo's app-UI gates. Builds are
# content-stamped, so a re-run with unchanged fixtures and an unchanged
# binary skips straight to the assertions. The no-site gates cost seconds
# either way.
#
# Usage:
#   bash scripts/render-gates.sh                       # everything (build + nobuild)
#   bash scripts/render-gates.sh --group nobuild        # only the gates that open no binary — no MOSS_BIN needed
#   bash scripts/render-gates.sh --group build          # only the gates that need MOSS_BIN
#   bash scripts/render-gates.sh --project chromium     # only each gate's chromium project (unfiltered if it defines none; SKIP if it defines projects but not this one)
#   bash scripts/render-gates.sh --list                 # print the selected gate names, one per line, and exit
#   bash scripts/render-gates.sh --group nobuild --list   # same, scoped to one group
#   bash scripts/render-gates.sh --check                # every playwright/*.config.ts is classified exactly once; exit 1 and name it otherwise
#   bash scripts/render-gates.sh comments-cascade       # one, by config name (positional args override the group)
#   MOSS_BIN=target/release/moss-cli bash scripts/render-gates.sh   # CI: reuse the artifact
set -euo pipefail
cd "$(dirname "$0")/.."

# Every gate whose config, spec, helper, or a playwright/vite.*-harness.config.ts
# named in its `webServer.command` reads MOSS_BIN or calls buildScratchSite.
# nav-island and edge-clamp build no scratch SITE — they serve a static vite
# harness — but that harness execs MOSS_BIN itself for its token table
# (`moss-cli describe --json`), so both belong here rather than in
# GATES_NOBUILD; same for vertical-nav-chrome, which serves the production
# theme.ts through vite for the same reason.
GATES_BUILD=(
  customization-cascade
  comments-cascade
  no-important-cascade
  dark-layer-order
  heading-weight
  reading-scale-order
  pre-paint-dark
  ui-accent-seam
  nav-toggle-cluster
  nav-mobile
  grid-mobile-collapse
  card-cover-ratio
  hero-caption
  hero-tone
  heading-anchor
  footnote-target
  share-card
  notebook-loads
  nav-island
  edge-clamp
  vertical-nav-chrome
)

# The rest: no config, spec, or harness in this gate's chain ever reads
# MOSS_BIN or calls buildScratchSite. These can start immediately — no build,
# no display wrapper, no artifact to wait for.
GATES_NOBUILD=(
  ambient-video
  grid-block-rhythm
  term-index-layout
  site-elevation
  card-media-track
  img-fallback
  vertical-measure
  article-date-alignment
  footer-subscribe-alignment
)

# One linear-search helper, not three copies of the same loop. No namerefs —
# this has to run under macOS's bash 3.2, not just CI's bash 5. `"$@"` after
# the `shift` is the haystack; a caller passing a possibly-empty array
# (EXCLUDED today) must expand it as `${arr[@]+"${arr[@]}"}`, not bare
# `"${arr[@]}"` — under `set -u`, bash 3.2 treats a zero-element array as
# unset and errors on the bare form.
contains() {
  local needle=$1
  shift
  local x
  for x in "$@"; do
    [ "$x" = "$needle" ] && return 0
  done
  return 1
}

# Configs deliberately not run by either array above, as "name:reason" pairs
# — `--check` treats one of these the same as a GATES_BUILD/GATES_NOBUILD
# membership, so every playwright/*.config.ts (bar the vite.* harnesses) is
# accounted for exactly once. Empty here: every site/cascade gate this repo
# carries a spec for is wired into one of the two arrays above. A gate the
# private repo has not wired anywhere was not ported at all (see the C2a
# report), so there is nothing orphaned here to excuse.
EXCLUDED=()

is_excluded_gate() {
  local candidate entry
  for entry in ${EXCLUDED[@]+"${EXCLUDED[@]}"}; do
    candidate="${entry%%:*}"
    [ "$candidate" = "$1" ] && return 0
  done
  return 1
}

# `--check`: every playwright/*.config.ts (excluding the vite.*-harness
# configs, which are spawned BY a gate's config rather than being one
# themselves) must be in exactly one of GATES_BUILD, GATES_NOBUILD, EXCLUDED.
# Zero means a config nobody classified — the "spec written, never wired"
# failure this whole split exists to end. Two means a gate double-counted,
# which would make --list's build/nobuild union larger than the config set.
check_classification() {
  local status=0 f base hits
  for f in playwright/*.config.ts; do
    base="$(basename "$f" .config.ts)"
    case "$base" in
      vite.*) continue ;;
    esac
    hits=0
    if contains "$base" ${GATES_BUILD[@]+"${GATES_BUILD[@]}"}; then hits=$((hits + 1)); fi
    if contains "$base" ${GATES_NOBUILD[@]+"${GATES_NOBUILD[@]}"}; then hits=$((hits + 1)); fi
    if is_excluded_gate "$base"; then hits=$((hits + 1)); fi
    if [ "$hits" -eq 0 ]; then
      echo "check: $base (playwright/${base}.config.ts) is in none of GATES_BUILD, GATES_NOBUILD, EXCLUDED" >&2
      status=1
    elif [ "$hits" -gt 1 ]; then
      echo "check: $base (playwright/${base}.config.ts) is in more than one of GATES_BUILD, GATES_NOBUILD, EXCLUDED" >&2
      status=1
    fi
  done
  if [ "$status" -eq 0 ]; then
    echo "check: every playwright/*.config.ts is classified exactly once"
  fi
  exit $status
}

# ---------------------------------------------------------------------------
# Argument parsing: --group build|nobuild, --project <name>, --list, --check,
# plus the pre-existing positional gate-name form, which still overrides
# everything else exactly as it did before this split.
GROUP=""
PROJECT=""
DO_LIST=false
DO_CHECK=false
POSITIONAL=()
while [ $# -gt 0 ]; do
  case "$1" in
    --group)
      [ $# -ge 2 ] || { echo "--group needs a value (build or nobuild)" >&2; exit 1; }
      GROUP="$2"
      shift 2
      ;;
    --project)
      [ $# -ge 2 ] || { echo "--project needs a value (a Playwright project name)" >&2; exit 1; }
      PROJECT="$2"
      shift 2
      ;;
    --list)
      DO_LIST=true
      shift
      ;;
    --check)
      DO_CHECK=true
      shift
      ;;
    *)
      POSITIONAL+=("$1")
      shift
      ;;
  esac
done

[ "$DO_CHECK" = true ] && check_classification

case "$GROUP" in
  "")
    GATES=("${GATES_BUILD[@]}" "${GATES_NOBUILD[@]}")
    ;;
  build)
    GATES=("${GATES_BUILD[@]}")
    ;;
  nobuild)
    GATES=("${GATES_NOBUILD[@]}")
    ;;
  *)
    echo "unknown --group '$GROUP' (want build or nobuild)" >&2
    exit 1
    ;;
esac
[ ${#POSITIONAL[@]} -gt 0 ] && GATES=("${POSITIONAL[@]}")

if [ "$DO_LIST" = true ]; then
  printf '%s\n' "${GATES[@]}"
  exit 0
fi

# MOSS_BIN is required only if the selected gates actually include one that
# reads it — that is the whole point of the split: `--group nobuild` must be
# able to start with no binary anywhere on the runner.
needs_bin=false
for gate in "${GATES[@]}"; do
  if contains "$gate" ${GATES_BUILD[@]+"${GATES_BUILD[@]}"}; then
    needs_bin=true
    break
  fi
done

if [ "$needs_bin" = true ]; then
  # CI points this at the release artifact `build-cli` already published, so
  # these gates cost an artifact download rather than a second compile of the
  # tree. Resolution order: MOSS_BIN env, then the debug binary, then release.
  if [ -z "${MOSS_BIN:-}" ]; then
    if [ -x "$PWD/target/debug/moss-cli" ]; then
      export MOSS_BIN="$PWD/target/debug/moss-cli"
    else
      export MOSS_BIN="$PWD/target/release/moss-cli"
    fi
  fi
  test -x "$MOSS_BIN" || {
    echo "no executable moss-cli at $MOSS_BIN — run \`cargo build -p moss-cli\` (or \`cargo build --release -p moss-cli\`), or set MOSS_BIN" >&2
    echo "  (most gates need no binary: bash scripts/render-gates.sh --group nobuild, or npx playwright test -c playwright/<name>.config.ts)" >&2
    exit 1
  }
fi

# A display wrapper is a Linux concern for a real windowed app; `moss-cli
# build` opens none (ADR-050), so nothing here needs xvfb even on Linux CI.

status=0
for gate in "${GATES[@]}"; do
  echo "── $gate ────────────────────────────────────────────"
  cfg="playwright/${gate}.config.ts"
  if [ -n "$PROJECT" ]; then
    # A config's project set decides how --project applies to it, and the
    # only reliable way to read that set is to ask Playwright, not to grep
    # the config: `--list --project=<name>` exits 0 when the config defines
    # that project, and its failure message on a miss names every project
    # the config DOES define — `Available projects: ""` for a config with no
    # named projects at all (this gate runs unfiltered, on whatever engine
    # its own `use:` picks), or a quoted list that does not include ours
    # (this gate has nothing to say about this engine — skip it).
    set +e
    probe="$(npx playwright test -c "$cfg" --list --project="$PROJECT" 2>&1)"
    probe_status=$?
    set -e
    if [ "$probe_status" -eq 0 ]; then
      npx playwright test -c "$cfg" --project="$PROJECT" --reporter=line || status=1
    elif printf '%s' "$probe" | grep -q 'Available projects: ""'; then
      npx playwright test -c "$cfg" --reporter=line || status=1
    else
      echo "SKIP $gate: no $PROJECT project"
    fi
  else
    npx playwright test -c "$cfg" --reporter=line || status=1
  fi
done
exit $status
