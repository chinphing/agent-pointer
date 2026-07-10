---
name: import-test-skill
description: Manual import test skill. Verifies zip import keeps scripts, references, and assets.
allowed-tools:
  - terminal
  - skill_read
---

# Import Test Skill

Use this skill to verify that **whole directory import** works after installing the zip.

## After import, check these files exist

Under `~/.pointer/skills/import-test-skill/`:

- `scripts/hello.sh`
- `scripts/check_env.py`
- `references/guide.md`
- `assets/demo.txt`
- `LICENSE.txt`

## Quick verification

1. Enable this skill in the Skills picker.
2. Run `skill_read` with `path=scripts/hello.sh` — should return the shell script text.
3. Run `skill_read` with `path=references/guide.md` — should return the guide.
4. In terminal: `bash {baseDir}/scripts/hello.sh` — should print a success message.

If any file is missing, directory import is broken.
