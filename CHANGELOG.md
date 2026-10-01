# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow the root `VERSION` file.

## [Unreleased]

### Added

- Build editions via `POINTER_EDITION` (`managed` or unset).
- Open-source repository files: license, security policy, contributing guide,
  code of conduct, and GitHub issue / PR templates.

### Changed

- Bundled Office/PDF skills that could not be redistributed were removed.
  Import equivalent skills yourself if you need them.
- Builds without the `managed` flavour do not default to readflowai.com,
  auto-update, usage reporting, or standalone license enforcement.
- The source tree no longer hardcodes Pointer's production domains. A `managed`
  build injects them at build time, and the build fails when the flavour is set
  without them (see `docs/en/deploy/editions.md`).
- The build flavour is named **`managed`**: any build that ships a control
  plane, whether it is Pointer's own release or an enterprise pointed at its
  internal hosts. `official` now describes Pointer's own release only and is no
  longer a valid `POINTER_EDITION` value — an unknown value fails the build
  instead of silently staying unbound.
- `docs/en/deploy/editions.md` documents all four build × runtime
  combinations; `docs/developer/README.md` carries the entry table.
- The public repository is named **agent-pointer**.

### Security

- Signing environment files and local logs must stay out of git.
  Rotate any Apple certificate password that was ever committed in history
  before publishing a public remote.
