# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow the root `VERSION` file.

## [Unreleased]

### Added

- Community and official build editions via `POINTER_EDITION`.
- Open-source repository files: license, security policy, contributing guide,
  code of conduct, and GitHub issue / PR templates.

### Changed

- Bundled Office/PDF skills that could not be redistributed were removed.
  Import equivalent skills yourself if you need them.
- Community builds do not default to readflowai.com, auto-update, usage
  reporting, or standalone license enforcement.
- The public repository is named **agent-pointer**.

### Security

- Signing environment files and local logs must stay out of git.
  Rotate any Apple certificate password that was ever committed in history
  before publishing a public remote.
