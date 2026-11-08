<script setup lang="ts">
/**
 * 模型服务的新建 / 编辑表单。
 *
 * 交互对齐客户端（官网）设置模型供应商的逻辑：**供应商和它的模型列表在同一个编辑器里**，
 * 不是「先建服务商、再去另一个页面逐个加模型」。
 *
 * 流程：选模板 → 自动带出地址 → 填密钥 → 测试连接（顺带拉回远端模型清单）
 *       → 在同一个列表里增删模型 → 保存（服务商 + 模型一起落库）。
 */
import { computed, ref } from 'vue'
import { useNotice } from '../lib/notice'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import providerCatalog from '../../../seed/providers.json'
import {
  LIMIT_KEYS,
  THINKING_KEYS,
  buildProviderDefaults,
  payloadToState,
} from '../lib/modelPayload'
import { FIELD_DEFAULTS, contextBudgetLabel } from '../lib/modelDefaults'

const props = defineProps<{ resource: any | null }>()
const { t } = useI18n()
const emit = defineEmits<{
  (e: 'saved'): void
  (e: 'cancel'): void
  /** 指定可见范围时，父组件拿 id 打开授权面板 */
  (e: 'created', id: string): void
}>()

/**
 * 模板来自 `admin/seed/providers.json` —— 和后端种子数据同一份文件。
 * 这里只列「非种子」的服务商：种子那三家（qwen/deepseek/豆包）已经建好了，
 * 新建时再出现一遍会让人以为要重复建。
 */
interface PresetModel {
  id: string
}
interface PresetProvider {
  slug: string
  name: string
  short?: string
  seeded: boolean
  baseUrl: string
  models: PresetModel[]
}
interface TemplateOption {
  id: string
  label: string
  /** 按钮上的短名，标签一行放得下 */
  short: string
  slug: string
  name: string
  baseUrl: string
  models: string[]
}

const catalog = (providerCatalog as { providers: PresetProvider[] }).providers
const TEMPLATES: TemplateOption[] = [
  ...catalog
    .filter((p) => !p.seeded)
    .map((p) => ({
      id: p.slug,
      label: p.name,
      short: p.short ?? p.name,
      slug: p.slug,
      name: p.name,
      baseUrl: p.baseUrl,
      models: p.models.map((m) => m.id),
    })),
  { id: 'custom', label: t('provider.tplCustom'), short: t('provider.tplCustomShort'), slug: '', name: '', baseUrl: '', models: [] },
]

/** 常用四个直接露出，其余收进「更多服务商」，避免一次铺满十几个。 */
const PRIMARY_TEMPLATE_IDS = new Set(['zhipu', 'kimi', 'openai', 'custom'])
const moreTemplatesOpen = ref(false)
const primaryTemplates = computed(() => TEMPLATES.filter((t) => PRIMARY_TEMPLATE_IDS.has(t.id)))
const restTemplates = computed(() => TEMPLATES.filter((t) => !PRIMARY_TEMPLATE_IDS.has(t.id)))
const visibleTemplates = computed(() => {
  if (moreTemplatesOpen.value) return [...primaryTemplates.value, ...restTemplates.value]
  const selected = restTemplates.value.find((t) => t.id === template.value)
  return selected ? [...primaryTemplates.value, selected] : primaryTemplates.value
})

const isEdit = computed(() => Boolean(props.resource))
const template = ref('bailian')
const form = ref({ slug: '', name: '', baseUrl: '', apiKey: '' })

/** 编辑时后端只说「有没有密钥」，不返明文——所以只能给脱敏提示，不能回显。 */
const hasSecret = ref(Boolean(props.resource?.hasSecret))
/** 后端只给脱敏值，明文仍只在「查看密钥」里。 */
const secretMask = ref<string | null>(props.resource?.secretMask ?? null)

/** 点进来时全选掩码，直接粘贴/输入就整段替换，不会粘成「掩码+新钥」。 */
function selectIfMask(e: FocusEvent) {
  const el = e.target as HTMLInputElement
  if (isMaskValue.value) el.select()
}

/** 框里还是那串脱敏值时，用明文显示（否则密码框会把 • 再打一遍码）。 */
const isMaskValue = computed(() => Boolean(secretMask.value) && form.value.apiKey === secretMask.value)

/**
 * 密钥提交规则（唯一入口，避免多处判断打架）：
 * - 框里还是脱敏值      → 不修改
 * - 清空且原本有密钥    → 确认后删除（secret: ''）
 * - 含 • 占位符         → 拦下，要求整段替换
 * - 其它非空            → 轮换
 */
function secretToSend(): { secret?: string; error?: string; cancel?: boolean } {
  const raw = form.value.apiKey.trim()
  if (raw === (secretMask.value ?? '')) return {}
  if (!raw) {
    if (!hasSecret.value) return {}
    if (!window.confirm(t('provider.secretClearConfirm'))) return { cancel: true }
    return { secret: '' }
  }
  if (raw.includes('•')) return { error: t('provider.secretMaskHint') }
  return { secret: raw }
}
const busy = ref(false)
const error = ref('')
const { notice, flash: flashNotice, clear: clearNotice } = useNotice()

const testing = ref(false)
const testResult = ref<{ ok: boolean; message: string } | null>(null)

/** 这个服务商下要保留的模型（新建时=要建的，编辑时=保存后仍存在的）。 */
const modelList = ref<string[]>([])
/** 编辑模式：slug → 已存在的模型资源 id，用于判断哪些是新增、哪些要删。 */
const existingIds = ref<Record<string, string>>({})
/** 编辑模式：本地已有的模型（用于和远端拉回来的做差集）。 */
const loaded = ref(false)

const newModel = ref('')
/** 可见范围（设计图 07）：全员直接授权；指定则建完后打开授权面板挑人。 */
const visibility = ref<'all' | 'pick'>('all')

/**
 * 服务商级默认参数：模型没单独配的字段就用这里的。
 * 图 07 没有这一段，但模型表单里写了「跟随服务商」，服务商这边必须真的能设。
 */
const showDefaults = ref(false)
const defState = payloadToState(props.resource?.payload || {})
const defLimits = ref<Record<string, string>>(defState.limits)
const defThinking = ref<Record<string, string>>(defState.thinking)
function ph(v: string | number) {
  return t('models.defaultIs', { v })
}
function intensityLabel(v: string) {
  return v ? t(`tier.thinking_${v}`) : t('models.unset')
}

const hasDefaults = computed(
  () =>
    Object.values(defLimits.value).some((v) => v !== '') ||
    Object.values(defThinking.value).some((v) => v !== '')
)

// 上一次由模板自动填进去的值：只有用户没动过才跟着模板换，
// 否则从「百炼」切到「DeepSeek」会留下一个叫「阿里云百炼」的 DeepSeek 服务商。
// 必须在下面那次 applyTemplate 之前声明 —— const 有暂时性死区，
// 函数声明会提升但读不到还没初始化的 const。
const autoFilled = ref({ slug: '', name: '' })

if (props.resource) {
  const payload = props.resource.payload || {}
  form.value = {
    slug: props.resource.slug || '',
    name: props.resource.name || '',
    baseUrl: payload.baseUrl || '',
    // 脱敏值直接放进输入框：看得出是哪把密钥，改就整段替换
    apiKey: props.resource.secretMask || '',
  }
  void loadExistingModels()
} else {
  applyTemplate('bailian')
}

/** 编辑模式：把该服务商已配的模型读进来，一起显示在列表里。 */
async function loadExistingModels() {
  try {
    const r = await api.get('/api/admin/resources?kind=model')
    const mine = (r.items || []).filter((m: any) => m.parentId === props.resource.id)
    modelList.value = mine.map((m: any) => m.slug)
    existingIds.value = Object.fromEntries(mine.map((m: any) => [m.slug, m.id]))
  } catch (e) {
    error.value = errText(e)
  } finally {
    loaded.value = true
  }
}

function applyTemplate(id: string) {
  template.value = id
  const t = TEMPLATES.find((x) => x.id === id)
  if (!t) return
  if (t.baseUrl) form.value.baseUrl = t.baseUrl
  // 模板自带模型清单：选中即填好，管理员只需要补 key
  if (t.models.length) {
    modelList.value = [...new Set([...modelList.value, ...t.models])]
  }
  const slugUntouched = !form.value.slug || form.value.slug === autoFilled.value.slug
  const nameUntouched = !form.value.name || form.value.name === autoFilled.value.name
  if (t.slug && slugUntouched) form.value.slug = t.slug
  if (t.name && nameUntouched) form.value.name = t.name
  autoFilled.value = { slug: t.slug, name: t.name }
  testResult.value = null
}

async function testConnection() {
  testing.value = true
  testResult.value = null
  flashNotice('')
  try {
    const r = await api.post('/api/admin/providers/test', {
      baseUrl: form.value.baseUrl.trim(),
      apiKey: form.value.apiKey.trim(),
    })
    testResult.value = { ok: Boolean(r.ok), message: r.message || (r.ok ? t('provider.connected') : t('provider.connectFailed')) }
    if (r.ok) {
      const remote: string[] = r.models || []
      const added = remote.filter((m) => !modelList.value.includes(m))
      if (remote.length === 0) {
        flashNotice(t('provider.noModels'))
      } else if (added.length === 0) {
        flashNotice(t('provider.fetchedAll', { n: remote.length }))
      } else {
        modelList.value = [...modelList.value, ...added]
        flashNotice(t('provider.fetchedAdded', { total: remote.length, added: added.length }))
      }
    }
  } catch (e) {
    testResult.value = { ok: false, message: errText(e) }
  } finally {
    testing.value = false
  }
}

function addModel() {
  const name = newModel.value.trim()
  if (!name) return
  if (modelList.value.includes(name)) {
    newModel.value = ''
    return
  }
  modelList.value.push(name)
  newModel.value = ''
}

function removeModel(slug: string) {
  modelList.value = modelList.value.filter((m) => m !== slug)
}

/** 保存前检查：被档位映射引用的模型不能直接删，否则客户端会拿到指向空模型的档位。 */
async function modelsReferencedByTiers(slugs: string[]): Promise<string[]> {
  if (!slugs.length) return []
  const r = await api.get('/api/admin/resources?kind=tier_profile')
  const hits: string[] = []
  for (const tp of r.items || []) {
    const raw = JSON.stringify(tp.payload ?? {})
    for (const s of slugs) {
      if (raw.includes(`"${s}"`)) hits.push(s)
    }
  }
  return [...new Set(hits)]
}

async function submit() {
  error.value = ''
  flashNotice('')
  if (!form.value.slug.trim()) return (error.value = t('provider.errSlug'))
  if (!form.value.name.trim()) return (error.value = t('provider.errName'))
  if (!form.value.baseUrl.trim()) return (error.value = t('provider.errBaseUrl'))

  busy.value = true
  try {
    const payload = {
      baseUrl: form.value.baseUrl.trim(),
      ...buildProviderDefaults({ limits: defLimits.value, thinking: defThinking.value }),
    }

    if (isEdit.value) {
      const removed = Object.keys(existingIds.value).filter((s) => !modelList.value.includes(s))
      if (removed.length) {
        const used = await modelsReferencedByTiers(removed)
        if (used.length) {
          error.value = t('provider.referenced', { list: used.join('、') })
          return
        }
      }

      const next = secretToSend()
      if (next.error) {
        error.value = next.error
        return
      }
      if (next.cancel) return

      await api.put(`/api/admin/resources/${props.resource.id}`, {
        name: form.value.name.trim(),
        payload,
        // 注意用 !== undefined：空字符串是「删除密钥」的有效值，不能用真值判断
        ...(next.secret !== undefined ? { secret: next.secret } : {}),
      })

      for (const slug of removed) {
        await api.del(`/api/admin/resources/${existingIds.value[slug]}`)
      }
      for (const slug of modelList.value) {
        if (existingIds.value[slug]) continue
        await api.post('/api/admin/resources', {
          kind: 'model',
          slug,
          name: slug,
          enabled: true,
          parentId: props.resource.id,
          payload: {},
        })
      }
      emit('saved')
      return
    }

    const created: any = await api.post('/api/admin/resources', {
      kind: 'provider',
      slug: form.value.slug.trim(),
      name: form.value.name.trim(),
      enabled: true,
      payload,
      secret: form.value.apiKey.trim() || undefined,
    })
    for (const slug of modelList.value) {
      await api.post('/api/admin/resources', {
        kind: 'model',
        slug,
        name: slug,
        enabled: true,
        parentId: created.id,
        payload: {},
      })
    }
    if (visibility.value === 'all') {
      await api.put(`/api/admin/resources/${created.id}/grants`, { items: [{ type: 'all' }] })
      emit('saved')
    } else {
      // 不给授权 = 谁都看不到，正好等管理员在授权面板里挑
      emit('created', created.id)
    }
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="modal-mask" @click.self="emit('cancel')">
    <div class="modal" style="width: 640px">
      <h2>{{ isEdit ? t('provider.editTitle') : t('provider.createTitle') }}</h2>

      <template v-if="!isEdit">
        <label>{{ t('provider.chooseTemplate') }}</label>
        <div class="tpl-grid" :class="{ expanded: moreTemplatesOpen }">
          <button
            v-for="item in visibleTemplates"
            :key="item.id"
            type="button"
            class="tpl"
            :class="{ active: template === item.id }"
            @click="applyTemplate(item.id)"
          >
            {{ item.short }}
          </button>
          <button
            v-if="restTemplates.length"
            type="button"
            class="tpl tpl-more"
            @click="moreTemplatesOpen = !moreTemplatesOpen"
          >
            {{ moreTemplatesOpen ? t('provider.fewerTemplates') : t('provider.moreTemplates', { n: restTemplates.length }) }}
          </button>
        </div>
      </template>

      <div class="two">
        <div>
          <label>{{ t('provider.name') }}</label>
          <input v-model="form.name" :placeholder="t('provider.namePlaceholder')" />
        </div>
        <div>
          <label>{{ t('provider.slug') }}</label>
          <input v-model="form.slug" :disabled="isEdit" placeholder="bailian" />
        </div>
      </div>

      <label>{{ t('provider.baseUrl') }}</label>
      <input v-model="form.baseUrl" class="mono" placeholder="https://…/v1" />
      <p class="small muted" style="margin: 4px 0 0">
        {{ t('provider.baseUrlHint') }}
      </p>

      <label>
        {{ t('provider.apiKey') }}
        <span v-if="hasSecret" class="tag ok" style="margin-left: 6px">{{ t('provider.secretMasked') }}</span>
      </label>
      <input
        v-model="form.apiKey"
        :type="isMaskValue ? 'text' : 'password'"
        class="mono"
        placeholder="sk-…"
        @focus="selectIfMask"
      />
      <p class="small muted" style="margin: 4px 0 0">{{ hasSecret ? t('provider.secretHint') : t('provider.secretHintNew') }}</p>

      <div class="row" style="gap: 10px; margin-top: 12px">
        <button class="tiny" :disabled="testing" @click="testConnection">
          {{ testing ? t('provider.testing') : t('provider.test') }}
        </button>
        <span
          v-if="testResult"
          class="small"
          :style="{ color: testResult.ok ? 'var(--ok)' : 'var(--danger)' }"
        >
          {{ testResult.message }}
        </span>
      </div>

      <!-- 模型列表：和供应商在同一个编辑器里，不再去「模型」页逐个加 -->
      <div class="models">
        <div class="spread" style="margin-bottom: 8px">
          <span class="small">
            {{ t('provider.models', { n: modelList.length }) }}
            <span v-if="isEdit" class="muted">{{ t('provider.removeHint') }}</span>
          </span>
        </div>

        <div class="row" style="gap: 8px; margin-top: 8px">
          <input
            v-model="newModel"
            class="tiny"
            style="flex: 1"
            :placeholder="t('provider.addPlaceholder')"
            @keyup.enter="addModel"
          />
          <button class="tiny" :disabled="!newModel.trim()" @click="addModel">{{ t('provider.add') }}</button>
        </div>

        <div v-if="!modelList.length && loaded" class="small muted" style="padding: 6px 2px">
          {{ t('provider.emptyModels') }}
        </div>

        <div class="model-list">
          <div v-for="m in modelList" :key="m" class="model-row">
            <span class="mono">{{ m }}</span>
            <button class="tiny" :title="t('provider.remove')" @click="removeModel(m)">{{ t('provider.remove') }}</button>
          </div>
        </div>

      </div>

      <template v-if="!isEdit">
        <label>{{ t('resources.visibility') }}</label>
        <div class="row" style="gap: 8px">
          <button
            type="button"
            class="pill"
            :class="{ active: visibility === 'all' }"
            @click="visibility = 'all'"
          >
            {{ t('resources.visAll') }}
          </button>
          <button
            type="button"
            class="pill"
            :class="{ active: visibility === 'pick' }"
            @click="visibility = 'pick'"
          >
            {{ t('resources.visPick') }}
          </button>
          <span v-if="visibility === 'pick'" class="small muted">{{ t('resources.pickAfterCreate') }}</span>
        </div>
      </template>

      <div class="defaults">
        <button type="button" class="defaults-toggle" @click="showDefaults = !showDefaults">
          <span>{{ showDefaults ? '▾' : '▸' }}</span>
          {{ t('provider.defaults') }}
          <span v-if="hasDefaults" class="tag ok" style="margin-left: 6px">{{ t('provider.defaultsSet') }}</span>
        </button>
        <template v-if="showDefaults">
        <p class="small muted" style="margin: 6px 0 0">{{ t('provider.defaultsHint') }}</p>
        <p class="small" style="margin: 4px 0 0; color: var(--warn)">{{ t('provider.defaultsOverride') }}</p>

        <div class="two" style="margin-top: 8px">
          <div>
            <label>{{ t('models.contextBudget') }}</label>
            <input v-model="defLimits.contextBudgetTokens" type="number" min="0" :placeholder="contextBudgetLabel()" />
          </div>
          <div>
            <label>{{ t('models.maxTokens') }}</label>
            <input v-model="defLimits.maxTokens" type="number" min="0" :placeholder="ph(FIELD_DEFAULTS.maxTokens)" />
          </div>
          <div>
            <label>{{ t('models.temperature') }}</label>
            <input v-model="defLimits.temperature" type="number" step="0.05" min="0" max="2" :placeholder="ph(FIELD_DEFAULTS.temperature)" />
          </div>
          <div>
            <label>{{ t('models.topP') }}</label>
            <input v-model="defLimits.topP" type="number" step="0.05" min="0" max="1" :placeholder="ph(FIELD_DEFAULTS.topP)" />
          </div>
        </div>

        <div class="three" style="margin-top: 8px">
          <div>
            <label>{{ t('tier.thinking') }}</label>
            <select v-model="defThinking.thinkingIntensity">
              <option value="">{{ t('models.intensityAuto') }}</option>
              <option v-for="v in ['off','low','medium','high','max']" :key="v" :value="v">{{ intensityLabel(v) }}</option>
            </select>
          </div>
          <div>
            <label>{{ t('models.reasoningInMessages') }}</label>
            <select v-model="defThinking.reasoningInMessages">
              <option value="">{{ t('models.noDefault') }}</option>
              <option value="true">{{ t('models.yes') }}</option>
              <option value="false">{{ t('models.no') }}</option>
            </select>
          </div>
        </div>
        </template>
      </div>

      <div v-if="error" class="err" style="margin-top: 12px">{{ error }}</div>
      <div v-if="notice" class="okmsg" style="margin-top: 12px">{{ notice }}</div>

      <div class="modal-actions">
        <button @click="emit('cancel')">{{ t('common.cancel') }}</button>
        <button class="primary" :disabled="busy" @click="submit">
          {{ isEdit ? t('common.save') : modelList.length ? t('provider.createWithModels', { n: modelList.length }) : t('provider.create') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tpl-grid { display: flex; gap: 6px; margin-top: 6px; }
.tpl-grid.expanded { display: grid; grid-template-columns: repeat(3, 1fr); }
.tpl { flex: 1 1 0; min-width: 0; text-align: center; font-size: 13px;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.tpl.active { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
.two { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
.models { margin-top: 16px; border: 1px solid var(--line); border-radius: var(--radius-sm); padding: 12px; }
.model-list { display: flex; flex-direction: column; gap: 2px; max-height: 220px; overflow: auto; }
.model-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 5px 6px;
  border-radius: 6px;
}
.model-row:hover { background: var(--panel-alt); }
.pill {
  font-size: 12px;
  padding: 4px 14px;
  border-radius: 13px;
  background: var(--panel);
  border: 1px solid var(--line);
  color: var(--muted);
}
.pill.active { background: #edf5fd; border-color: var(--accent); color: var(--accent); }
.defaults {
  margin-top: 14px;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 8px 12px 12px;
}
.defaults-toggle {
  border: none;
  background: transparent;
  padding: 0;
  font-size: 12px;
  color: var(--muted);
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 4px;
}
.defaults-toggle:hover { color: var(--fg); }
.three { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; }
.tpl-more { border-style: dashed; color: var(--muted); }
</style>
