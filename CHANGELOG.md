# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow the root `VERSION` file.

## [Unreleased]

### Added

- Build editions via `POINTER_EDITION` (`official` or unset).
- Open-source repository files: license, security policy, contributing guide,
  code of conduct, and GitHub issue / PR templates.

### Changed

- Bundled Office/PDF skills that could not be redistributed were removed.
  Import equivalent skills yourself if you need them.
- Builds without the `official` flavor do not default to readflowai.com,
  auto-update, usage reporting, or standalone license enforcement.
- The source tree no longer hardcodes Pointer's production domains. An `official`
  build injects them at build time, and the build fails when the flavour is set
  without them (see `docs/contributing/editions.md`).
- The public repository is named **agent-pointer**.

### Security

- Signing environment files and local logs must stay out of git.
  Rotate any Apple certificate password that was ever committed in history
  before publishing a public remote.
