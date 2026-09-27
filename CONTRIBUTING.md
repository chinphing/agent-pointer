# Contributing

Please read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before participating.

## Issues and pull requests

- Use the bug template for defects. Say whether you used an **official** signed
  installer or a **community** build from source, plus OS and version.
- Use the feature template for requests.
- Security reports go to [SECURITY.md](SECURITY.md), not public issues.

## Developer Certificate of Origin

Commits must include:

```text
Signed-off-by: Your Name <you@example.com>
```

`git commit -s` adds this line. By signing off you certify the contribution
is yours to submit under the Apache License 2.0.

## Local development

See [DEVELOPMENT.md](DEVELOPMENT.md) for environment setup and
`tauri:dev` / `server:dev` / `web:dev`.

```bash
npm install
npm test
cargo test --workspace
```

Leave `POINTER_EDITION` unset for everyday local work. Set
`POINTER_EDITION=community` only when you need community-build defaults.
Details: [docs/contributing/editions.md](docs/contributing/editions.md).

## Version numbers

Change only the root `VERSION` file, then run `npm run version:sync`.
Do not hand-edit `package.json` or `tauri.conf.json` versions.
See [docs/contributing/versioning.md](docs/contributing/versioning.md).

## Documentation

- User tutorials: [docs/user/](docs/user/README.md)
- Developer docs: [docs/developer/](docs/developer/README.md)
- Build and packaging: [docs/contributing/](docs/contributing/README.md)
