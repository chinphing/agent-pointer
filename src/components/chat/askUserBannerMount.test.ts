// @vitest-environment node
//
// Where the ask_user banner is mounted is a product contract, not an implementation
// detail: it belongs above the input box, in the composer's auxiliary stack next to the
// background-job / outbound-queue bars — not at the top of the message list. Mounting
// Composer in a test would drag in the whole chat store, so pin the wiring at the SFC
// level: `Composer.vue` mounts it, `ChatView.vue` must not.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

function sfc(name: string): string {
  return readFileSync(new URL(`./${name}`, import.meta.url), 'utf8')
}

describe('ask_user banner mount point', () => {
  it('is mounted by Composer.vue above the input box', () => {
    const composer = sfc('Composer.vue')
    expect(composer).toContain("import AskUserBanner from './AskUserBanner.vue'")
    expect(composer).toContain('<AskUserBanner />')
    // Above the input shell, in the same stack as the job / queue bars.
    expect(composer.indexOf('<AskUserBanner />')).toBeLessThan(
      composer.indexOf('ref="composerDropZoneRef"')
    )
  })

  it('is not mounted at the top of the message list (ChatView.vue)', () => {
    expect(sfc('ChatView.vue')).not.toContain('<AskUserBanner')
  })
})
