#!/usr/bin/env bash
# Sync docx / pptx / xlsx from https://github.com/anthropics/skills (main).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="${TMPDIR:-/tmp}/anthropics-skills-sync"
rm -rf "$TMP"
git clone --depth 1 --filter=blob:none --sparse https://github.com/anthropics/skills.git "$TMP"
(
  cd "$TMP"
  git sparse-checkout set skills/docx skills/pptx skills/xlsx
)
for skill in docx pptx xlsx; do
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
done < <(cd "$TMP" && git ls-files -s skills/docx skills/pptx skills/xlsx)
echo "Synced docx, pptx, xlsx from anthropics/skills@main into $ROOT/skills/"
