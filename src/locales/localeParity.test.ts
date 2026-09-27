import { describe, expect, it } from 'vitest'
import en from './en.json'
import zhCN from './zh-CN.json'

function flattenKeys(obj: unknown, prefix = ''): string[] {
  if (obj == null || typeof obj !== 'object' || Array.isArray(obj)) {
    return prefix ? [prefix] : []
  }
  const entries = Object.entries(obj as Record<string, unknown>)
  if (entries.length === 0) return prefix ? [prefix] : []
  return entries.flatMap(([k, v]) => {
    const path = prefix ? `${prefix}.${k}` : k
    if (v != null && typeof v === 'object' && !Array.isArray(v)) {
      return flattenKeys(v, path)
    }
    return [path]
  })
}

describe('locale key parity', () => {
  const zhKeys = new Set(flattenKeys(zhCN))
  const enKeys = new Set(flattenKeys(en))

  it('zh-CN and en share the same key set', () => {
    const missingInEn = [...zhKeys].filter(k => !enKeys.has(k)).sort()
    const missingInZh = [...enKeys].filter(k => !zhKeys.has(k)).sort()
    expect({ missingInEn, missingInZh }).toEqual({ missingInEn: [], missingInZh: [] })
  })

  it('has core navigation and language keys', () => {
    for (const key of [
      'settings.language',
      'settings.sections.generation',
      'shell.newTask',
      'chat.composerPlaceholder',
      'agents.general'
    ]) {
      expect(zhKeys.has(key), `missing ${key}`).toBe(true)
    }
  })

  it('has P3 workspace / skills / auth / updater keys', () => {
    for (const key of [
      'workspace.filesTab',
      'workspace.debugTerminal',
      'skills.searchPlaceholder',
      'skills.importZip',
      'auth.browserLogin',
      'auth.sessionExpired',
      'updater.readyTitle',
      'errors.uploadAborted'
    ]) {
      expect(zhKeys.has(key), `missing ${key}`).toBe(true)
      expect(enKeys.has(key), `missing en ${key}`).toBe(true)
    }
  })

  it('has mop-up leftovers keys (chat/settings secondary copy)', () => {
    for (const key of [
      'chat.changeBlocksCount',
      'chat.markMilestone',
      'chat.videoConfirmCost',
      'chat.argsParseFailed',
      'chat.paused',
      'chat.md.copyConfig',
      'chat.md.chartPending',
      'chat.computer.pickScreen',
      'chat.computer.planning',
      'chat.composerDefault',
      'chat.workPrefix',
      'chat.newConversation',
      'settings.cron.onceIn',
      'settings.cron.weekday0',
      'settings.channels.imPairing',
      'settings.automation.legacyTokenBanner',
      'settings.models.confirmDeleteTitle',
      'settings.runtimeParams.extraBodyMustObject',
      'settings.thinking.unset',
      'settings.tiers.fast',
      'common.continueAnyway'
    ]) {
      expect(zhKeys.has(key), `missing ${key}`).toBe(true)
      expect(enKeys.has(key), `missing en ${key}`).toBe(true)
    }
  })
})
