// @vitest-environment happy-dom

import { createApp, nextTick, reactive, ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../../i18n'
import type { ProviderConfig } from '../../types/chat'

const storeState = vi.hoisted(() => {
  const { reactive } = require('vue') as typeof import('vue')
  const settings = reactive({
    providers: [] as ProviderConfig[],
    activeProviderId: 'qwen',
    model: 'qwen3.5-plus',
    temperature: 0.3,
    maxTokens: 64_000
  })
  return {
    settings,
    saveModelService: vi.fn(async () => undefined),
    removeProvider: vi.fn((id: string) => {
      const remaining = settings.providers.filter(provider => provider.id !== id)
      if (settings.activeProviderId === id) {
        const replacement = remaining.find(provider => provider.models.length > 0)
        if (!replacement) return
        settings.activeProviderId = replacement.id
        if (!replacement.models.includes(settings.model)) settings.model = replacement.models[0]
      }
      settings.providers = remaining
    }),
    addProvider: vi.fn(),
    updateProvider: vi.fn(),
    setActiveProvider: vi.fn()
  }
})

const platformAuthState = vi.hoisted(() => {
  const { ref } = require('vue') as typeof import('vue')
  return { isStandalone: ref(false) }
})

vi.mock('../../stores/settings', () => ({
  useSettingsStore: () => storeState
}))

vi.mock('../../stores/platformAuth', () => ({
  usePlatformAuthStore: () => ({
    get isStandalone() {
      return platformAuthState.isStandalone.value
    }
  })
}))

vi.mock('../../composables/useRuntimeParams', () => ({
  DEFAULT_CONTEXT_BUDGET_TOKENS: 262_144,
  DEFAULT_MODEL_MAX_TOKENS: 64_000,
  DEFAULT_MODEL_TEMPERATURE: 0.3,
  buildCustomModelEntryFromProvider: vi.fn(),
  patchProviderModelCapability: vi.fn(),
  pruneInheritedModelConfigs: vi.fn((value: unknown) => value),
  sanitizeProviderModelConfigs: vi.fn((value: unknown) => value),
  useRuntimeParams: () => ({
    params: ref({}),
    patch: vi.fn(),
    reset: vi.fn()
  })
}))

import ModelServiceSection from './ModelServiceSection.vue'

const mountedApps: Array<ReturnType<typeof createApp>> = []

function provider(id: string, source?: 'user' | 'platform'): ProviderConfig {
  return {
    id,
    name: id === 'qwen' ? '千问' : '本地模型',
    baseUrl: id === 'qwen' ? 'https://dashscope.aliyuncs.com/compatible-mode/v1' : 'http://127.0.0.1:8000/v1',
    apiKey: `${id}-key`,
    models: [id === 'qwen' ? 'qwen3.5-plus' : 'local-model'],
    modelConfigs: {},
    source
  }
}

async function settle() {
  await Promise.resolve()
  await nextTick()
  await Promise.resolve()
  await nextTick()
}

function mountSection() {
  const host = document.createElement('div')
  document.body.append(host)
  const app = createApp(ModelServiceSection, {
    form: { platformReadOnly: ref(false) }
  })
  app.use(i18n)
  mountedApps.push(app)
  app.mount(host)
  return host
}

beforeEach(() => {
  i18n.global.locale.value = 'zh-CN'
  platformAuthState.isStandalone.value = false
  storeState.settings.providers = [provider('qwen', 'platform'), provider('local')]
  storeState.settings.activeProviderId = 'qwen'
  storeState.settings.model = 'qwen3.5-plus'
  vi.clearAllMocks()
})

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

describe('ModelServiceSection', () => {
  it('only exposes delete for custom providers and waits for confirmation before persisting', async () => {
    const host = mountSection()
    await settle()

    expect(host.querySelector('[aria-label="删除 千问"]')).toBeNull()
    const removeCustom = host.querySelector<HTMLButtonElement>('[aria-label="删除 本地模型"]')
    expect(removeCustom).toBeTruthy()

    removeCustom!.click()
    await settle()

    expect(document.body.textContent).toContain('确认删除模型服务')
    expect(document.body.textContent).toContain('本地模型')
    expect(storeState.removeProvider).not.toHaveBeenCalled()
    expect(storeState.saveModelService).not.toHaveBeenCalled()

    ;[...document.body.querySelectorAll<HTMLButtonElement>('button')]
      .find(button => button.textContent === '确认删除')!
      .click()
    await settle()

    expect(storeState.removeProvider).toHaveBeenCalledWith('local')
    expect(storeState.saveModelService).toHaveBeenCalledWith(expect.objectContaining({
      activeProviderId: 'qwen',
      model: 'qwen3.5-plus',
      providers: [expect.objectContaining({ id: 'qwen', apiKey: '', source: 'platform' })]
    }))
  })

  it('does not modify state when deletion confirmation is cancelled', async () => {
    const host = mountSection()
    await settle()

    host.querySelector<HTMLButtonElement>('[aria-label="删除 本地模型"]')!.click()
    await settle()
    ;[...document.body.querySelectorAll<HTMLButtonElement>('button')]
      .find(button => button.textContent === '取消')!
      .click()
    await settle()

    expect(storeState.settings.providers.map(provider => provider.id)).toEqual(['qwen', 'local'])
    expect(storeState.removeProvider).not.toHaveBeenCalled()
    expect(storeState.saveModelService).not.toHaveBeenCalled()
  })

  it('restores the custom provider when persistence fails', async () => {
    storeState.saveModelService.mockRejectedValueOnce(new Error('disk unavailable'))
    const host = mountSection()
    await settle()

    host.querySelector<HTMLButtonElement>('[aria-label="删除 本地模型"]')!.click()
    await settle()
    ;[...document.body.querySelectorAll<HTMLButtonElement>('button')]
      .find(button => button.textContent === '确认删除')!
      .click()
    await settle()

    expect(storeState.settings.providers.map(provider => provider.id)).toEqual(['qwen', 'local'])
    expect(storeState.settings.activeProviderId).toBe('qwen')
    expect(storeState.settings.model).toBe('qwen3.5-plus')
    expect(host.textContent).toContain('删除服务失败，请重试')
  })

  it('lets standalone edit and delete all providers like custom services', async () => {
    platformAuthState.isStandalone.value = true
    const host = mountSection()
    await settle()

    expect(host.textContent).toContain('本实例模型服务')
    expect(host.textContent).not.toContain('只读')
    expect(host.querySelector('[aria-label="删除 千问"]')).toBeTruthy()
    expect(host.querySelector('[aria-label="编辑 千问"]')).toBeTruthy()
    expect(host.querySelector('[aria-label="编辑 本地模型"]')).toBeTruthy()
  })
})
