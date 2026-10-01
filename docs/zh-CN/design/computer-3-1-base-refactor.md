# Computer 3.1 Base Refactor

## Scope

This refactor updates the Computer Primary tier to match 3.1 base behavior:

- Primary now consumes two or three labeled images per turn:
  optional before-action screen, after-action screen, annotated after-action screen.
- Nearby references remain capped to 10 entries and are injected together with pointer position.
- Verify policy uses expected vs unexpected change:
  expected change maps to pass,
  unexpected change or no-obvious-change maps to fail.
- Repetition policy follows count buckets:
  0 continue,
  1-3 switch tactic,
  >3 escalate.
- Primary routing supports both index and coordinate methods in one tier.

## Implementation Notes

- Screen payload assembly is updated in screen inject:
  Primary labels and image list now include before/after/annotated ordering.
- Primary capture pipeline now stores prior unmarked screenshots so before-action inject can be built.
- Primary overlay work now keeps raw marked output to provide after-action full screen evidence.
- Tool prompt routing now introduces a hybrid positioning mode for Primary.
- Runtime repetition block includes explicit policy hints to reduce model ambiguity.
- Primary v1 thoughts output is intentionally concise:
  external thoughts only include core reasoning conclusions,
  while MA templates are treated as internal reasoning aids.
  This rule currently applies only to Primary, not Intermediate/Advanced.
- Tier upgrade signal source is switched to sidecar:
  host now reads `verify:report` (`action_result`, `repetition_count`)
  and performs internal threshold judgment for upgrade,
  instead of parsing upgrade signal from thoughts text lines.

## Compatibility

- Platform compatibility remains unchanged:
  capture, annotation, and input backends are shared across macOS, Windows, and Linux paths.
- Entry compatibility remains unchanged:
  behavior is implemented in pointer-core runtime and applies to both app and web entry flows.

## Risk

- First-capture turns can omit before-action image by design.
  Verify must treat before/after comparison as n/a in that case.
- Hybrid route increases tool choice surface.
  Prompt constraints must keep index/coordinate arguments mutually exclusive per call.
