# Pointer

Cross-platform AI workstation: desktop (Tauri 2 + Vue 3) and web (`pointer-server` + the same UI). Core chat, tools, Skills, and agents live in `crates/pointer-core`.

[简体中文](README.zh-CN.md)

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

## Editions

| Build | What you get |
| --- | --- |
| **Official** signed installer | Optional readflowai.com login, auto-update, usage reporting, cloud host, standalone license on official server packages |
| **Community** from this repo | Same app; you bring the model API key. No default official cloud, updater, or license gate |

See [docs/user/editions.md](docs/user/editions.md).

## Documentation

| Audience | Start here |
| --- | --- |
| Users | [docs/user/](docs/user/README.md) |
| Developers | [docs/developer/](docs/developer/README.md) |
| Contributors | [CONTRIBUTING.md](CONTRIBUTING.md) · [DEVELOPMENT.md](DEVELOPMENT.md) |
| Security | [SECURITY.md](SECURITY.md) |
| Changes | [CHANGELOG.md](CHANGELOG.md) |

Full index: [docs/README.md](docs/README.md).

## Develop

```bash
npm install
npm run tauri:dev          # desktop
npm run server:dev         # then npm run web:dev for the browser UI
npm test
cargo test --workspace
```

Leave `POINTER_EDITION` unset for local work. Details: [DEVELOPMENT.md](DEVELOPMENT.md).

## License

Apache License 2.0. Trademarks and official domains are described in [NOTICE](NOTICE).
