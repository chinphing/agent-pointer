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
  combinations; `docs/zh-CN/developer/README.md` carries the entry table.
- The public repository is named **agent-pointer**.

### Fixed

- The `ask_user` question card renders again, above the input box (same stack as
  the background-job / outbound-queue bars; it renders the standard ask_user card
  unchanged, with no extra shell of its own), and only for **sub-agent** questions: the lead agent's own
  card is already inline in the transcript. `ChatView.vue` mounted
  `<AskUserBanner />` without importing it, so Vue fell back to an inert
  `<askuserbanner>` element: a question asked by a sub-agent had no surface once
  the frames stopped keeping pending `ask_user` cards.
- Sub-agent stats lines now fold every tool the named buckets do not cover
  (`ask_user`, `task_board_*`, `read_lints`, …) into「其他 N 次」instead of reporting
  「工具 0 次」, and give `job` / `run_subagent` their own「后台任务」/「委派」buckets — so a
  spawn that only asked a question no longer collapses to the bare「过程」placeholder.

- The sub-agent `ask_user` bar mounts only once its question is parseable
  (`askUserQuestionIsDrawable`): a call whose arguments are still streaming used to
  show an empty bar above the composer.
- `scripts/check-vue-template-imports.mjs` fails `npm test` when a template
  renders a component the SFC never imports (self-references and Vue built-ins
  excluded) — `vue-tsc` does not catch that case.

### Security

- Signing environment files and local logs must stay out of git.
  Rotate any Apple certificate password that was ever committed in history
  before publishing a public remote.
