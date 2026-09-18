#!/usr/bin/env bash
# check-sc-demos.sh — verify sc-demo blocks are consistent
#
# Each :::grid 2 {.sc-demo} block has a left cell (```markdown source)
# and a right cell (live shortcode using ::::). This script asserts that
# the source shown matches the live shortcode — normalizing 4-colon fences
# in the right cell to 3-colon for comparison with the left cell.
#
# Usage: scripts/check-sc-demos.sh [file...]
#   No args: checks all site/docs/writing/shortcodes/*.md
#   With args: checks only those files

set -euo pipefail

DOCS_DIR="$(cd "$(dirname "$0")/.." && pwd)/site/docs/writing/shortcodes"
FILES=()
for arg in "$@"; do FILES+=("$arg"); done

if [ "$#" -eq 0 ]; then
  while IFS= read -r f; do FILES+=("$f"); done < <(find "$DOCS_DIR" -name "*.md" | sort)
fi

errors=0

for file in "${FILES[@]}"; do
  in_demo=0
  in_left=0
  in_code_fence=0
  left=""
  right=""
  demo_start_line=0
  lineno=0

  while IFS= read -r line || [[ -n "$line" ]]; do
    lineno=$((lineno + 1))

    # Detect sc-demo opener
    if [[ "$line" == ":::grid 2 {.sc-demo}" ]]; then
      in_demo=1
      in_left=1
      in_code_fence=0
      left=""
      right=""
      demo_start_line=$lineno
      continue
    fi

    if [[ $in_demo -eq 0 ]]; then
      continue
    fi

    # Detect outer closer (bare :::, not inside code fence, not in right cell shortcode)
    if [[ "$line" == ":::" && $in_code_fence -eq 0 && $in_left -eq 0 ]]; then
      # End of demo block — normalize and compare
      # Right cell uses :::: fences; normalize to ::: for comparison
      normalized_right="${right//$'::::\n'/$':::\n'}"
      normalized_right="${normalized_right#::::}"
      # Also handle first line starting with ::::
      norm_right=""
      while IFS= read -r r || [[ -n "$r" ]]; do
        norm_right+="${r/::::/:::}"$'\n'
      done <<< "$right"
      # Strip all trailing newlines from both sides before comparing
      left_stripped="${left}"
      while [[ "${left_stripped: -1}" == $'\n' ]]; do left_stripped="${left_stripped%$'\n'}"; done
      while [[ "${norm_right: -1}" == $'\n' ]]; do norm_right="${norm_right%$'\n'}"; done

      if [[ "$left_stripped" != "$norm_right" ]]; then
        echo "FAIL: $file (sc-demo at line $demo_start_line)"
        echo "  Left cell (source):"
        while IFS= read -r l; do echo "    $l"; done <<< "$left_stripped"
        echo "  Right cell (normalized):"
        while IFS= read -r r; do echo "    $r"; done <<< "$norm_right"
        errors=$((errors + 1))
      fi

      in_demo=0
      in_left=0
      in_code_fence=0
      left=""
      right=""
      continue
    fi

    # Detect +++ cell divider — only when outside code fence and in left cell
    if [[ "$line" == "+++" && $in_code_fence -eq 0 && $in_left -eq 1 ]]; then
      in_left=0
      continue
    fi

    if [[ $in_left -eq 1 ]]; then
      # Track markdown code fence open/close
      if [[ "$line" =~ ^\`\`\`markdown$ ]]; then
        in_code_fence=1
        continue
      fi
      if [[ $in_code_fence -eq 1 && "$line" == '```' ]]; then
        in_code_fence=0
        continue
      fi
      if [[ $in_code_fence -eq 1 ]]; then
        left+="$line"$'\n'
      fi
    else
      # Right cell: collect everything (the :::: closer is not added since
      # we stop at the outer ::: closer above; but the right cell's own
      # :::: closer needs to be included for normalization)
      right+="$line"$'\n'
    fi

  done < "$file"
done

if [[ $errors -gt 0 ]]; then
  echo ""
  echo "$errors sc-demo block(s) have mismatched source and live shortcode."
  echo "Edit the left cell (markdown code block) to match the right cell shortcode."
  exit 1
else
  echo "All sc-demo blocks consistent. (${#FILES[@]} file(s) checked)"
fi
