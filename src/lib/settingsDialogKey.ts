import type { InjectionKey } from 'vue'

/**
 * Open the settings dialog, optionally jumping to a section
 * (e.g. 'assistant' = 智能体面板).
 * Provided by AppShell; consumed by chat UI (ChatTopBar / Composer).
 */
export const OpenSettingsKey: InjectionKey<(section?: string) => void> = Symbol.for(
  'pointer.openSettings'
)
