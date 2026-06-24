#!/usr/bin/env bash
# Sync docx / pptx / xlsx / pdf from https://github.com/anthropics/skills (main).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="${TMPDIR:-/tmp}/anthropics-skills-sync"
rm -rf "$TMP"
git clone --depth 1 --filter=blob:none --sparse https://github.com/anthropics/skills.git "$TMP"
(
  cd "$TMP"
  git sparse-checkout set skills/docx skills/pptx skills/xlsx skills/pdf
)
for skill in docx pptx xlsx pdf; do
  rm -rf "$ROOT/skills/$skill"
  cp -R "$TMP/skills/$skill" "$ROOT/skills/$skill"
done
# Restore executable bits from upstream git modes
while IFS= read -r line; do
  mode="${line%% *}"
  path="${line#* }"
  rel="${path#skills/}"
  if [[ "$mode" == "100755" && -f "$ROOT/skills/$rel" ]]; then
    chmod +x "$ROOT/skills/$rel"
  fi
done < <(cd "$TMP" && git ls-files -s skills/docx skills/pptx skills/xlsx skills/pdf)
echo "Synced docx, pptx, xlsx, pdf from anthropics/skills@main into $ROOT/skills/"
echo "Note: skills/pdf/SKILL.md and skills/pdf/reference.md include Pointer-specific sections — merge after sync."
