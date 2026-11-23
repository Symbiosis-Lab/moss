#!/usr/bin/env bash
# Run the CSS cascade / theme render gates for published-page CSS.
#
# Some of them build a throwaway moss site with `target/debug/moss-cli`
# (built by `bash scripts/build-moss-cli.sh`: a plain cargo build leaves out
# the browser editor, and its gate fails),
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
# cascade).
# App/editor/preview gates stay in the private desktop repo, because their
# subject is the desktop app's own chrome, not anything this repo builds.
#
# `moss-cli build` opens no window, so no display wrapper is needed
# even on Linux — unlike the desktop repo's app-UI gates. Builds are
# content-stamped, so a re-run with unchanged fixtures and an unchanged
# binary skips straight to the assertions. The no-site gates cost seconds
# either way.
#
# Usage:
#   bash scripts/render-gates.sh                       # everything (build + nobuild)
#   bash scripts/render-gates.sh --group nobuild        # only the gates that open no binary — no MOSS_BIN needed
#   bash scripts/render-gates.sh --group build          # only the gates that need MOSS_BIN
#   bash scripts/render-gates.sh --project chromium     # only each gate's chromium project (SKIP for a gate that doesn't run this engine)
#   bash scripts/render-gates.sh --list                 # print the selected gate names, one per line, and exit
#   bash scripts/render-gates.sh --group nobuild --list   # same, scoped to one group
#   bash scripts/render-gates.sh --select               # skip each gate whose inputs are byte-for-byte a green run's (scripts/render-gate-select.mjs)
#   bash scripts/render-gates.sh --jobs 6               # six gates at a time, one browser each; see "Sharding" below
#   bash scripts/render-gates.sh --jobs auto            # as many as RENDER_GATES_MEM_GB allows; see "Sharding" below
#   bash scripts/render-gates.sh --check                # every playwright/*.config.ts is classified exactly once; exit 1 and name it otherwise
#   bash scripts/render-gates.sh comments-cascade       # one, by config name (positional args override the group)
#   MOSS_BIN=target/release/moss-cli bash scripts/render-gates.sh   # point at a release build instead of the debug default
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
  hosted-bold
  reading-scale-order
  pre-paint-dark
  ui-accent-seam
  nav-toggle-cluster
  nav-split-order
  nav-split-pack
  nav-nowrap
  nav-flattened
  nav-one-row
  nav-first-paint
  nav-current-mark
  header-hit-areas
  lightbox-github-shapes
  grid-card-no-cover
  link-card-cover-slot
  grid-tile-scope
  nav-mobile
  nav-submenu
  grid-mobile-collapse
  collage
  card-cover-ratio
  card-cover-auto
  lead-listing
  listing-picture-strip
  stream-listing
  marquee
  field-place-end
  grid-card-image-inline-size
  hero-caption
  hero-links
  hero-from
  hero-pause
  hero-caption-stack
  content-width-escape
  hero-tone
  theme-floor-hooks
  hero-overlay-legibility
  heading-anchor
  footnote-target
  share-card
  notebook-loads
  nav-island
  contents-ruler
  contents-lens
  header-none
  page-chrome-none
  header-scroll
  folded-callouts
  header-over-ink
  header-over-band
  edge-clamp
  vertical-nav-chrome
  video-embed-shape
  hls-short-clip
  place-map-tokens
  country-map
  country-map-interaction
  place-map-align
  place-map-cut
  cluster-layer-zoom
  place-map-route
  place-dot-links
  preview-parity-gate
  plain-tab-reload
  missing-file-box
  touch-targets
  places-explorer-boot
  places-explorer-camera
  places-explorer-ring
  places-explorer-cards
  places-explorer-labels
  places-explorer-chip
  places-explorer-embed
  places-explorer-real
  places-explorer-perf
  places-explorer-vertical
  places-explorer-single
  footer-end-space
  colophon-credit
  request-form
  facts-margin
  closing-picture
  content-grid
  wcag-output
  archival-footer
  subscribed-state
  type-defaults
  diagrams
  browser-editor
  focus-point
  address-bar
  progress-line
  series-nav
  card-min-token
)

# The rest: no config, spec, or harness in this gate's chain ever reads
# MOSS_BIN or calls buildScratchSite. These can start immediately — no build,
# no display wrapper, no artifact to wait for.
GATES_NOBUILD=(
  ambient-video
  bridge-capture
  iframe-open-url
  grid-block-rhythm
  term-index-layout
  site-elevation
  card-media-track
  card-grid-rows
  card-cover-fit
  card-description-clamp
  img-fallback
  vertical-measure
  vertical-sizes
  screen-page
  color-scheme
  article-date-alignment
  footer-subscribe-alignment
  float-mobile-collapse
  prose-spacing-ladder
  round-marks
  modal-alignment
)

# Gates that assert timing which fails under load; --jobs runs them alone.
SERIAL_GATES=(marquee places-explorer-ring browser-editor focus-point address-bar footnote-target progress-line)

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
  for base in "${SERIAL_GATES[@]}"; do
    if ! contains "$base" ${GATES_BUILD[@]+"${GATES_BUILD[@]}"} ${GATES_NOBUILD[@]+"${GATES_NOBUILD[@]}"}; then
      echo "check: SERIAL_GATES names $base, which is in neither GATES_BUILD nor GATES_NOBUILD" >&2
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
SELECT=false
JOBS=1
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
    --select)
      SELECT=true
      shift
      ;;
    --jobs)
      [ $# -ge 2 ] || { echo "--jobs needs a value (a number or auto)" >&2; exit 1; }
      JOBS="$2"
      shift 2
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
  # CI compiles its own debug CLI right before calling this script (`cargo
  # build -p moss-cli` in .github/workflows/render-gates.yml) rather than
  # downloading a release artifact from another workflow — see that job's
  # comment for why. Resolution order here: MOSS_BIN env, then the debug
  # binary, then release.
  if [ -z "${MOSS_BIN:-}" ]; then
    if [ -x "$PWD/target/debug/moss-cli" ]; then
      export MOSS_BIN="$PWD/target/debug/moss-cli"
    else
      export MOSS_BIN="$PWD/target/release/moss-cli"
    fi
  fi
  test -x "$MOSS_BIN" || {
    echo "no executable moss-cli at $MOSS_BIN — run \`bash scripts/build-moss-cli.sh\` (or \`bash scripts/build-moss-cli.sh --release\`), or set MOSS_BIN" >&2
    echo "  (most gates need no binary: bash scripts/render-gates.sh --group nobuild, or npx playwright test -c playwright/<name>.config.ts)" >&2
    exit 1
  }
fi

# A display wrapper is a Linux concern for a real windowed app; `moss-cli
# build` opens none, so nothing here needs xvfb even on Linux CI.

# Under --select, a gate runs with every Node process recording what it reads,
# and a green run stores that input set; a gate whose set hashes the same as a
# green run's is skipped. A built scratch site is an input, so every selected
# build-group gate builds its sites first, GATE_BUILD_JOBS (default 4) at a
# time: `--list` loads the config, which builds, and each site's stamp lock
# keeps two gates from building the same site at once.
pw() {
  if [ "$SELECT" = true ]; then
    GATE_INPUTS_OUT="$inputs" NODE_OPTIONS="--require $PWD/tests/support/gate-inputs.cjs${NODE_OPTIONS:+ $NODE_OPTIONS}" \
      npx playwright test "$@"
  else
    npx playwright test "$@"
  fi
}
record_green() {
  [ "$SELECT" = true ] || return 0
  # A gate whose inputs cannot be stored simply runs again next time.
  GATE_HEAD="$(git rev-parse --short=10 HEAD 2>/dev/null || true)" node scripts/render-gate-select.mjs record "$gate" "$PROJECT" "$inputs" || true
}

if [ "$SELECT" = true ]; then
  for gate in "${GATES[@]}"; do
    contains "$gate" ${GATES_BUILD[@]+"${GATES_BUILD[@]}"} && echo "$gate"
  done | xargs -P "${GATE_BUILD_JOBS:-4}" -I{} sh -c 'npx playwright test -c "playwright/{}.config.ts" --list > /dev/null 2>&1' || true
fi

# Sharding. With --jobs N above 1, N gates run at a time, each Playwright run
# with one worker, so the machine holds at most N browsers whatever the configs
# ask for. Each lane takes the next gate as it frees up, longest first by the
# durations the last run recorded in target/render-gate-times.tsv. The gates in
# SERIAL_GATES assert timing that fails under load, so they run alone, one after
# another, once the rest are done. Each run keeps its logs in a folder under
# target/test-tmp/render-gates, named on the lanes' header line, and prints them in
# the order of GATES when it ends. A fullyParallel config run alone keeps four
# workers, or one per WORKER_GB a smaller RENDER_GATES_MEM_GB allows
# (RENDER_GATES_WORKERS, read by playwright/define-gate-config.ts). A
# RENDER_GATES_WORKERS already in the environment wins over the grant.
#
# RENDER_GATES_MEM_GB is the memory the run may use (default half the
# machine's). One lane needs LANE_FIRST_GB, since the heaviest gate peaks near
# that at one worker; each further lane needs LANE_MORE_GB, a lane's measured
# share when lanes run together. That caps --jobs N, and --jobs auto takes as
# many lanes as it allows, one per CPU at most.
WORKER_GB=2 LANE_FIRST_GB=9 LANE_MORE_GB=4
TIMES="${RENDER_GATES_TIMES:-target/render-gate-times.tsv}"
granted="${RENDER_GATES_MEM_GB:-}"
if [ -z "${RENDER_GATES_MEM_GB:-}" ] && [ "$JOBS" != 1 ]; then
  if [ -r /proc/meminfo ]; then RENDER_GATES_MEM_GB=$(awk '/^MemTotal:/ { print int($2 / 2097152) }' /proc/meminfo)
  else RENDER_GATES_MEM_GB=$(( $(sysctl -n hw.memsize) / 2147483648 )); fi
fi
max_lanes=$(awk -v m="${RENDER_GATES_MEM_GB:-0}" -v h="$LANE_FIRST_GB" -v l="$LANE_MORE_GB" 'BEGIN { n = 1 + int((m - h) / l); print (n < 1 ? 1 : n) }')
case "$JOBS" in
  auto)
    JOBS=$max_lanes
    cpus=$(getconf _NPROCESSORS_ONLN)
    [ "$JOBS" -gt "$cpus" ] && JOBS=$cpus
    ;;
  *[!0-9]* | "" | 0) echo "--jobs wants a positive number or auto, not '$JOBS'" >&2; exit 1 ;;
esac
if [ "$JOBS" -gt "$max_lanes" ]; then
  echo "── --jobs $JOBS needs more than RENDER_GATES_MEM_GB=$RENDER_GATES_MEM_GB ($LANE_FIRST_GB GB for one lane, $LANE_MORE_GB more for each further one): running $max_lanes" >&2
  JOBS=$max_lanes
fi
# A grant also caps the browsers a gate run alone may start.
workers=$(awk -v g="${granted:-8}" -v w="$WORKER_GB" 'BEGIN { n = int(g / w); print (n > 4 ? 4 : (n < 1 ? 1 : n)) }')
export RENDER_GATES_WORKERS="${RENDER_GATES_WORKERS:-$workers}"

# One gate: its Playwright run(s), then exit 0 green, 1 red, 2 skipped by --select.
run_gate() {
  # pw and record_green read inputs through bash's dynamic scope.
  local gate=$1 cfg="playwright/$1.config.ts" list_output="" result=0 inputs=""
  echo "── $gate ────────────────────────────────────────────"
  if [ "$SELECT" = true ]; then
    if node scripts/render-gate-select.mjs check "$gate" "$PROJECT"; then return 2; fi
    inputs="$(mktemp -t render-gate-inputs)"
  fi
  if [ -n "$PROJECT" ]; then
    # Ask Playwright for the configured project set rather than grepping the
    # config. Only its explicit missing-project diagnostic means this gate has
    # nothing to say about the selected engine; config-load and scratch-build
    # errors must remain failures instead of becoming false skips.
    if list_output="$(npx playwright test -c "$cfg" --list --project="$PROJECT" 2>&1)"; then
      if pw -c "$cfg" --project="$PROJECT" --reporter=line; then record_green; else result=1; fi
    else
      case "$list_output" in
        *"Error: Project(s) \"$PROJECT\" not found. Available projects:"*)
          echo "SKIP $gate: no $PROJECT project"
          ;;
        *)
          printf '%s\n' "$list_output" >&2
          echo "ERROR $gate: Playwright project listing failed" >&2
          result=1
          ;;
      esac
    fi
  else
    if pw -c "$cfg" --reporter=line; then record_green; else result=1; fi
  fi
  [ -n "$inputs" ] && rm -f "$inputs"
  return $result
}

# Each run its own folder, so two runs in one checkout never read each other's results.
mkdir -p "${RENDER_GATES_LOGS:-target/test-tmp/render-gates}"
logs="$(mktemp -d "${RENDER_GATES_LOGS:-target/test-tmp/render-gates}/run.XXXXXX")"
# One gate, with "<exit code> <seconds>" in its .result beside its log. Above one lane its
# output goes to the log and a line says when it is done.
run_logged() {
  local gate=$1 t0 rc=0 secs
  t0=$(date +%s)
  if [ "$JOBS" -gt 1 ]; then run_gate "$gate" > "$logs/$gate.log" 2>&1 || rc=$?
  else run_gate "$gate" || rc=$?; fi
  secs=$(( $(date +%s) - t0 ))
  echo "$rc $secs" > "$logs/$gate.result"
  [ "$JOBS" -le 1 ] || echo "── done $gate: exit $rc, $secs s" >&2
}

started=$(date +%s)
if [ "$JOBS" -le 1 ]; then
  for gate in "${GATES[@]}"; do run_logged "$gate"; done
else
  serial=() spread=()
  for gate in "${GATES[@]}"; do
    if contains "$gate" "${SERIAL_GATES[@]}"; then serial+=("$gate"); else spread+=("$gate"); fi
  done
  echo "── ${#spread[@]} gates on $JOBS lanes, one worker each, then ${#serial[@]} alone; logs in $logs"
  export -f run_logged run_gate pw record_green
  export SELECT PROJECT JOBS logs
  pool() { RENDER_GATES_WORKERS=1 xargs -P "$JOBS" -I{} "$BASH" -c 'run_logged "$1"' _ {} < "$logs/queue"; }
  serial_tail() { for gate in "$@"; do run_logged "$gate"; done; }
  # Job control gives each phase its own process group, so an interrupt ends every Playwright it started.
  group=""
  in_group() { set -m; "$@" & group=$!; set +m; wait "$group" || true; group=""; }
  trap '[ -z "$group" ] || kill -TERM -- "-$group" 2>/dev/null; exit 130' INT
  trap '[ -z "$group" ] || kill -TERM -- "-$group" 2>/dev/null; exit 143' TERM
  if [ ${#spread[@]} -gt 0 ]; then
    # A gate with no recorded time counts 60 s.
    { cat "$TIMES" 2>/dev/null || true; printf '=\t%s\n' "${spread[@]}"; } |
      awk -F'\t' '$1 != "=" { t[$1] = $2; next } { print ($2 in t ? t[$2] : 60) "\t" $2 }' | sort -rn | cut -f2 > "$logs/queue"
    in_group pool
  fi
  [ ${#serial[@]} -eq 0 ] || in_group serial_tail "${serial[@]}"
  trap - INT TERM
  for gate in "${GATES[@]}"; do [ -f "$logs/$gate.log" ] && cat "$logs/$gate.log"; done
fi

status=0 ran=0 skipped=0 slow=""
for gate in "${GATES[@]}"; do
  # A gate with no result never finished: a killed lane, or a pool xargs aborted.
  read -r rc secs 2>/dev/null < "$logs/$gate.result" || { rc=1 secs=""; }
  case "$rc" in
    0) ran=$((ran + 1)) ;;
    2) skipped=$((skipped + 1)); continue ;;
    *) ran=$((ran + 1)); status=1; echo "RED $gate" ;;
  esac
  [ -z "$secs" ] || slow="$slow$gate	$secs
"
done
# Keep the newest duration of every gate that ran, for the next run's plan.
mkdir -p "$(dirname "$TIMES")"
{ printf '%s' "$slow"; cat "$TIMES" 2>/dev/null || true; } | awk -F'\t' 'NF == 2 && !seen[$1]++' > "$TIMES.new" && mv "$TIMES.new" "$TIMES"
echo "── $ran ran in $(( $(date +%s) - started )) s on $JOBS lane(s); logs in $logs"
[ "$SELECT" = true ] && echo "── selected: ran $ran, skipped $skipped of ${#GATES[@]}"
exit $status
