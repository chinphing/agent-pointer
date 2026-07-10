# Skill import manual test

## Zip file

`import-test-skill.zip` — install via Skills picker → **导入 zip**.

Rebuild zip:

```bash
cd test-fixtures && zip -r import-test-skill.zip import-test-skill
```

## Expected layout inside zip

```
import-test-skill/
├── SKILL.md
├── LICENSE.txt
├── scripts/
│   ├── hello.sh
│   └── check_env.py
├── references/
│   └── guide.md
└── assets/
    └── demo.txt
```

## After import

Files should appear at:

`~/.pointer/skills/import-test-skill/`

## Checklist

- [ ] Skill **import-test-skill** appears in the list
- [ ] `scripts/hello.sh` exists on disk
- [ ] `scripts/check_env.py` exists on disk
- [ ] `references/guide.md` exists on disk
- [ ] `assets/demo.txt` exists on disk
- [ ] `skill_read` can load `references/guide.md`
