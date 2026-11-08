<script setup lang="ts">
/**
 * 档位映射矩阵（设计图 08/09）。
 *
 * 配的是「场景 × 档位 → 模型」，不是「员工用哪一档」——选哪一档仍由员工决定。
 * 下发形状见设计文档 §7.2：`{agentModeLlm:{场景:{fast|standard|expert:{providerId,model}}}, mediaUnderstanding:{类型:{providerId,model}}}`。
 *
 * 这是**配置矩阵**，不是表单：一眼要能看出哪些格子被改过、哪些还在继承。
 * 所以每格默认是「文字 + 幽灵下拉」，未配置显示灰色「继承默认」，配置了才显示模型名；
 * 保存放在底部、有改动才点亮。
 */
import { computed, ref, watch } from 'vue'
import { useNotice } from '../lib/notice'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'

const { t } = useI18n()

const props = defineProps<{
  resource: any | null
  models: any[]
  providers: any[]
  canWrite: boolean
}>()
const emit = defineEmits<{ (e: 'saved'): void }>()

/** 按用途分组：对话/工具走 agentModeLlm（三档），媒体走 mediaUnderstanding（单模型）。 */
const GROUPS = [
  {
    labelKey: 'tier.groupChat',
    scenes: [
      { key: 'general', labelKey: 'tier.scene_general', group: 'agentModeLlm' },
      { key: 'coder', labelKey: 'tier.scene_coder', group: 'agentModeLlm' },
      { key: 'computer', labelKey: 'tier.scene_computer', group: 'agentModeLlm' },
      { key: 'web_search', labelKey: 'tier.scene_web_search', group: 'agentModeLlm' },
    ],
  },
  {
    labelKey: 'tier.groupMedia',
    scenes: [
      { key: 'image', labelKey: 'tier.scene_image', group: 'mediaUnderstanding' },
      { key: 'audio', labelKey: 'tier.scene_audio', group: 'mediaUnderstanding' },
      { key: 'video', labelKey: 'tier.scene_video', group: 'mediaUnderstanding' },
    ],
  },
]

const TIERS = [
  { key: 'fast', labelKey: 'tier.tier_fast' },
  { key: 'standard', labelKey: 'tier.tier_standard' },
  { key: 'expert', labelKey: 'tier.tier_expert' },
]

/**
 * 电脑操控内部的定位链三段。下发形状（客户端 `settings.rs` 解析）：
 *   computerPipelineLlm.locate / locateVerify / verify = {providerId, model, thinkingIntensity?}
 *   computerPipelineLlm.byTier[档位].{locate,locateVerify,verify} 同上，留空继承全局
 * 老的 decision / position 两个槽位客户端仍兼容，但界面上不露出。
 */
const SLOTS = [
  { key: 'locate', labelKey: 'tier.slot_locate', hintKey: 'tier.slot_locateHint' },
  { key: 'locateVerify', labelKey: 'tier.slot_locateVerify', hintKey: 'tier.slot_locateVerifyHint' },
  { key: 'verify', labelKey: 'tier.slot_verify', hintKey: 'tier.slot_verifyHint' },
]

/** 与客户端 `thinkingIntensity` 阶梯一致；留空 = 继承模型默认。 */
const THINKING = [
  { key: '', label: '—' },
  { key: 'off', labelKey: 'tier.thinking_off' },
  { key: 'low', labelKey: 'tier.thinking_low' },
  { key: 'medium', labelKey: 'tier.thinking_medium' },
  { key: 'high', labelKey: 'tier.thinking_high' },
  { key: 'max', labelKey: 'tier.thinking_max' },
]

const payload = ref<any>({})
const saved = ref('{}')
const error = ref('')
const { notice, flash: flashNotice, clear: clearNotice } = useNotice()
const busy = ref(false)

watch(
  () => props.resource,
  (r) => {
    // 深拷贝：直接改 props 里的对象会污染列表数据。
    payload.value = r ? JSON.parse(JSON.stringify(r.payload ?? {})) : {}
    saved.value = stable(payload.value)
    error.value = ''
  },
  { immediate: true }
)

const providerSlugById = computed(() => {
  const map: Record<string, string> = {}
  for (const p of props.providers || []) map[p.id] = p.slug
  return map
})

/**
 * 可选模型。挂在不存在的服务商下面的模型要排除 —— 选了它会写进
 * `providerId: ""`，客户端拿到一个没有服务商的档位，等于配了个死档。
 */
const modelOptions = computed(() =>
  (props.models || [])
    .filter((m: any) => providerSlugById.value[m.parentId])
    .map((m: any) => ({
      slug: m.slug,
      label: m.name || m.slug,
      providerId: providerSlugById.value[m.parentId],
    }))
)

/** 媒体理解是「类型 → 单模型」，没有档位层；其它分组按档位取。 */
const FLAT_GROUP = 'mediaUnderstanding'

function cell(scene: string, group: string, tier: string): any {
  if (group === FLAT_GROUP) return payload.value?.[group]?.[scene] || {}
  return payload.value?.[group]?.[scene]?.[tier] || {}
}

function modelOf(scene: string, group: string, tier: string): string {
  return cell(scene, group, tier).model || ''
}

function intensityOf(scene: string, group: string, tier: string): string {
  return cell(scene, group, tier).thinkingIntensity || ''
}

function setModel(scene: string, group: string, tier: string, slug: string) {
  if (!payload.value[group]) payload.value[group] = {}
  if (group === FLAT_GROUP) {
    if (!slug) {
      delete payload.value[group][scene]
    } else {
      const found = modelOptions.value.find((m) => m.slug === slug)
      const prevIntensity = payload.value[group][scene]?.thinkingIntensity
      payload.value[group][scene] = {
        providerId: found?.providerId || '',
        model: slug,
        ...(prevIntensity ? { thinkingIntensity: prevIntensity } : {}),
      }
    }
    if (!Object.keys(payload.value[group]).length) delete payload.value[group]
    return
  }
  if (!payload.value[group][scene]) payload.value[group][scene] = {}
  if (!slug) {
    delete payload.value[group][scene][tier]
  } else {
    const found = modelOptions.value.find((m) => m.slug === slug)
    const prevIntensity = payload.value[group][scene][tier]?.thinkingIntensity
    payload.value[group][scene][tier] = {
      providerId: found?.providerId || '',
      model: slug,
      ...(prevIntensity ? { thinkingIntensity: prevIntensity } : {}),
    }
  }
  // 空对象留着会下发一堆空档位，清掉更干净。
  if (!Object.keys(payload.value[group][scene]).length) delete payload.value[group][scene]
  if (!Object.keys(payload.value[group]).length) delete payload.value[group]
}

function setIntensity(scene: string, group: string, tier: string, value: string) {
  const cur =
    group === FLAT_GROUP ? payload.value?.[group]?.[scene] : payload.value?.[group]?.[scene]?.[tier]
  if (!cur) return
  if (value) cur.thinkingIntensity = value
  else delete cur.thinkingIntensity
}

function clearCell(scene: string, group: string, tier: string) {
  setModel(scene, group, tier, '')
}

// ---- 电脑操控定位链 ----

function pipeCell(slot: string, col: string): any {
  const root = payload.value?.computerPipelineLlm
  if (!root) return {}
  return col ? root.byTier?.[col]?.[slot] || {} : root[slot] || {}
}

function pipeModel(slot: string, col: string): string {
  return pipeCell(slot, col).model || ''
}

function pipeIntensity(slot: string, col: string): string {
  return pipeCell(slot, col).thinkingIntensity || ''
}

function setPipeModel(slot: string, col: string, slug: string) {
  if (!payload.value.computerPipelineLlm) payload.value.computerPipelineLlm = {}
  const root = payload.value.computerPipelineLlm
  const target = col ? (root.byTier ??= {})[col] ??= {} : root
  if (!slug) {
    delete target[slot]
  } else {
    const found = modelOptions.value.find((m) => m.slug === slug)
    const prev = target[slot]?.thinkingIntensity
    target[slot] = {
      providerId: found?.providerId || '',
      model: slug,
      ...(prev ? { thinkingIntensity: prev } : {}),
    }
  }
  // 空对象不下发，省得客户端收到一堆空档位
  if (col && root.byTier && !Object.keys(root.byTier[col] || {}).length) delete root.byTier[col]
  if (root.byTier && !Object.keys(root.byTier).length) delete root.byTier
  if (!Object.keys(root).length) delete payload.value.computerPipelineLlm
}

function setPipeIntensity(slot: string, col: string, value: string) {
  const cur = col
    ? payload.value?.computerPipelineLlm?.byTier?.[col]?.[slot]
    : payload.value?.computerPipelineLlm?.[slot]
  if (!cur) return
  if (value) cur.thinkingIntensity = value
  else delete cur.thinkingIntensity
}

/** 已配置格子数：给用户一个「改了多少」的量化反馈。 */
const filledCount = computed(() => {
  let n = 0
  for (const g of GROUPS) for (const s of g.scenes) for (const t of TIERS) {
    if (modelOf(s.key, s.group, t.key)) n++
  }
  return n
})

const totalCells = GROUPS.reduce((n, g) => n + g.scenes.length, 0) * TIERS.length

/** 定位链已配置的格数（全局 3 + 三档各 3）。 */
const pipeFilled = computed(() => {
  let n = 0
  for (const sl of SLOTS) {
    if (pipeModel(sl.key, '')) n++
    for (const t of TIERS) if (pipeModel(sl.key, t.key)) n++
  }
  return n
})

/** 键序无关的稳定序列化：服务端存的键序和这里重建的不一定一致。 */
function stable(v: any): string {
  if (Array.isArray(v)) return `[${v.map(stable).join(',')}]`
  if (v && typeof v === 'object') {
    return `{${Object.keys(v)
      .sort()
      .map((k) => `${JSON.stringify(k)}:${stable(v[k])}`)
      .join(',')}}`
  }
  return JSON.stringify(v)
}

const dirty = computed(() => stable(payload.value) !== saved.value)

async function save() {
  if (!props.resource) return
  busy.value = true
  error.value = ''
  flashNotice('')
  try {
    await api.put(`/api/admin/resources/${props.resource.id}`, { payload: payload.value })
    saved.value = stable(payload.value)
    flashNotice(t('tier.saved'))
    emit('saved')
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

function revert() {
  payload.value = JSON.parse(saved.value)
  flashNotice('')
  error.value = ''
}

async function createDefault() {
  busy.value = true
  error.value = ''
  try {
    await api.post('/api/admin/resources', {
      kind: 'tier_profile',
      slug: 'default',
      name: t('tier.title'),
      enabled: true,
      payload: {},
    })
    emit('saved')
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="panel matrix-panel">
    <div v-if="error" class="err">{{ error }}</div>

    <div v-if="!resource" class="empty">
      {{ t('tier.empty') }}
      <button v-if="canWrite" class="tiny" style="margin-left: 8px" :disabled="busy" @click="createDefault">
        {{ t('tier.createDefault') }}
      </button>
    </div>

    <template v-else>
      <p class="hint">{{ t('tier.hint') }}</p>

      <div class="matrix-scroll">
      <div class="matrix">
        <div class="head-row">
          <div class="scene-col">{{ t('tier.scene') }}</div>
          <div v-for="tier in TIERS" :key="tier.key" class="tier-head">{{ t(tier.labelKey) }}</div>
        </div>

        <template v-for="g in GROUPS" :key="g.labelKey">
          <div class="group-row">{{ t(g.labelKey) }}</div>
          <div v-for="s in g.scenes" :key="s.group + s.key" class="scene-row">
            <div class="scene-col" :title="s.key">{{ t(s.labelKey) }}</div>
            <div
              v-if="s.group === FLAT_GROUP"
              class="cell flat"
              :class="{ filled: !!modelOf(s.key, s.group, '') }"
            >
              <select
                class="ghost"
                :class="{ filled: !!modelOf(s.key, s.group, '') }"
                :value="modelOf(s.key, s.group, '')"
                :disabled="!canWrite"
                @change="setModel(s.key, s.group, '', ($event.target as HTMLSelectElement).value)"
              >
                <option value="">{{ t('tier.inherit') }}</option>
                <option v-for="m in modelOptions" :key="m.slug" :value="m.slug">{{ m.label }}</option>
              </select>
              <select
                v-if="modelOf(s.key, s.group, '')"
                class="mini"
                :value="intensityOf(s.key, s.group, '')"
                :disabled="!canWrite"
                :title="t('tier.thinking')"
                @change="setIntensity(s.key, s.group, '', ($event.target as HTMLSelectElement).value)"
              >
                <option v-for="o in THINKING" :key="o.key" :value="o.key">
                  {{ o.labelKey ? t(o.labelKey) : o.label }}
                </option>
              </select>
              <button
                v-if="modelOf(s.key, s.group, '') && canWrite"
                class="clear"
                :title="t('tier.clearCell')"
                @click="clearCell(s.key, s.group, '')"
              >×</button>
            </div>
            <div
              v-for="tier in s.group === FLAT_GROUP ? [] : TIERS"
              :key="tier.key"
              class="cell"
              :class="{ filled: !!modelOf(s.key, s.group, tier.key) }"
            >
              <select
                class="ghost"
                :class="{ filled: !!modelOf(s.key, s.group, tier.key) }"
                :value="modelOf(s.key, s.group, tier.key)"
                :disabled="!canWrite"
                @change="setModel(s.key, s.group, tier.key, ($event.target as HTMLSelectElement).value)"
              >
                <option value="">{{ t('tier.inherit') }}</option>
                <option v-for="m in modelOptions" :key="m.slug" :value="m.slug">{{ m.label }}</option>
              </select>
              <select
                v-if="modelOf(s.key, s.group, tier.key)"
                class="mini"
                :value="intensityOf(s.key, s.group, tier.key)"
                :disabled="!canWrite"
                :title="t('tier.thinking')"
                @change="setIntensity(s.key, s.group, tier.key, ($event.target as HTMLSelectElement).value)"
              >
                <option v-for="o in THINKING" :key="o.key" :value="o.key">
                  {{ o.labelKey ? t(o.labelKey) : o.label }}
                </option>
              </select>
              <button
                v-if="modelOf(s.key, s.group, tier.key) && canWrite"
                class="clear"
                :title="t('tier.clearCell')"
                @click="clearCell(s.key, s.group, tier.key)"
              >×</button>
            </div>
          </div>
        </template>
      </div>

      <div class="pipe-block">
        <div class="pipe-title">
          <h3>{{ t('tier.pipeTitle') }}</h3>
          <span class="small muted">
            {{ t('tier.pipeIntro') }}
          </span>
        </div>

        <div class="pipe-grid pipe-head">
          <div class="slot-col">{{ t('tier.slot') }}</div>
          <div class="global-col">{{ t('tier.globalCol') }}</div>
          <div v-for="tier in TIERS" :key="tier.key">{{ t(tier.labelKey) }}</div>
        </div>

        <div v-for="sl in SLOTS" :key="sl.key" class="pipe-grid pipe-row">
          <div class="slot-col" :title="t(sl.hintKey)">{{ t(sl.labelKey) }}</div>
          <div
            v-for="col in ['', ...TIERS.map((t) => t.key)]"
            :key="col || 'global'"
            class="cell"
            :class="{ filled: !!pipeModel(sl.key, col), 'global-col': col === '' }"
          >
            <select
              class="ghost"
              :class="{ filled: !!pipeModel(sl.key, col) }"
              :value="pipeModel(sl.key, col)"
              :disabled="!canWrite"
              :title="col ? t('tier.pipeTierHint') : t('tier.pipeGlobalHint')"
              @change="setPipeModel(sl.key, col, ($event.target as HTMLSelectElement).value)"
            >
              <option value="">{{ col ? t('tier.pipeInherit') : t('tier.pipeFollowMain') }}</option>
              <option v-for="m in modelOptions" :key="m.slug" :value="m.slug">{{ m.label }}</option>
            </select>
            <select
              v-if="pipeModel(sl.key, col)"
              class="mini"
              :value="pipeIntensity(sl.key, col)"
              :disabled="!canWrite"
              :title="t('tier.thinking')"
              @change="setPipeIntensity(sl.key, col, ($event.target as HTMLSelectElement).value)"
            >
              <option v-for="o in THINKING" :key="o.key" :value="o.key">
                {{ o.labelKey ? t(o.labelKey) : o.label }}
              </option>
            </select>
            <button
              v-if="pipeModel(sl.key, col) && canWrite"
              class="clear"
              :title="t('tier.clearCell')"
              @click="setPipeModel(sl.key, col, '')"
            >×</button>
          </div>
        </div>
      </div>

      </div>

      <div class="foot">
        <span class="small muted">
          {{ t('tier.matrixCount', { n: filledCount, total: totalCells }) }}
          <template v-if="pipeFilled"> · {{ t('tier.pipeCount', { n: pipeFilled }) }}</template>
          <template v-if="dirty"> · {{ t('tier.dirty') }}</template>
        </span>
        <div v-if="canWrite" class="row" style="gap: 8px">
          <button v-if="dirty" class="tiny" :disabled="busy" @click="revert">{{ t('tier.revert') }}</button>
          <button class="primary" :disabled="busy || !dirty" @click="save">{{ t('tier.save') }}</button>
        </div>
      </div>

      <div v-if="notice" class="okmsg" style="margin-top: 10px">{{ notice }}</div>
    </template>
  </div>
</template>

<style scoped>
.matrix-panel { padding: 0; }
.hint { margin: 0; padding: 14px 18px; color: var(--muted); font-size: 12.5px; line-height: 1.7; border-bottom: 1px solid var(--line); }

.matrix-scroll { max-height: calc(100vh - 320px); overflow-y: auto; }
.matrix { display: flex; flex-direction: column; }

.head-row,
.scene-row {
  display: grid;
  grid-template-columns: 160px repeat(3, minmax(0, 1fr));
  align-items: center;
  gap: 10px;
  padding: 0 18px;
}
.head-row {
  position: sticky;
  top: 0;
  z-index: 4;
  height: 32px;
  font-size: 12px;
  color: var(--muted);
  background: var(--panel);
  border-bottom: 1px solid var(--line);
}
.tier-head { padding-left: 7px; }

.group-row {
  padding: 12px 18px 5px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.08em;
  color: #3f3f46;
  background: var(--panel-alt);
  border-top: 1px solid var(--line);
}

.scene-row { height: 38px; border-bottom: 1px solid var(--line); }
.scene-row:hover { background: #fafbfc; }
.scene-row:nth-child(even) { background: #fcfcfd; }
.scene-row:hover { background: #f7f9fb; }
.scene-col { font-size: 13.5px; }

.cell { display: flex; align-items: center; gap: 6px; min-width: 0; }
.cell.flat { grid-column: span 3; }

/* 幽灵下拉：平时看起来像一行文字，hover/聚焦才显出输入框 */
.ghost {
  flex: 1;
  min-width: 0;
  border: 1px solid transparent;
  background: transparent;
  padding: 4px 6px;
  font-size: 13px;
  color: var(--muted);
  border-radius: 6px;
  text-overflow: ellipsis;
}
.ghost:hover:not(:disabled) { border-color: var(--line-strong); background: #fff; }
.ghost:focus { background: #fff; }
/* 空态要看起来像「没填」，不是「已选中」 */
.ghost:not(.filled) { font-style: italic; color: var(--placeholder); }
.ghost.filled { color: var(--fg); font-weight: 500; }

/* 已配置的格子给一个极淡的强调条，扫一眼就知道哪些被改过 */
.cell.filled::before {
  content: '';
  flex: 0 0 3px;
  align-self: stretch;
  margin: 5px 0;
  border-radius: 2px;
  background: var(--accent);
}

.mini {
  flex: 0 0 50px;
  width: 50px;
  padding: 3px 2px;
  font-size: 11.5px;
  color: var(--muted);
  border-color: transparent;
  background: var(--panel-alt);
  border-radius: 6px;
}
.mini:hover { border-color: var(--line-strong); }

.clear {
  flex: 0 0 auto;
  border: 0;
  background: transparent;
  color: var(--placeholder);
  padding: 0 2px;
  font-size: 15px;
  line-height: 1;
  opacity: 0;
  transition: opacity 0.12s;
}
.cell:hover .clear { opacity: 1; }
.clear:hover { color: var(--danger); }

.pipe-block { border-top: 1px solid var(--line); background: var(--bg); }
.pipe-title { display: flex; align-items: baseline; gap: 10px; padding: 14px 18px 6px; }
.pipe-title h3 { margin: 0; font-size: 13px; }

.pipe-grid {
  display: grid;
  grid-template-columns: 112px 1.15fr repeat(3, minmax(0, 1fr));
  align-items: center;
  gap: 10px;
  padding: 0 18px;
}
.pipe-head { height: 28px; font-size: 12px; color: var(--muted); }
.pipe-head > div { padding-left: 7px; }
.pipe-row { height: 38px; border-top: 1px solid var(--line); }
.pipe-row:hover { background: #f7f9fb; }
.slot-col { font-size: 13.5px; }
/* 全局默认是基准值，不是第四个档位——用底色和分隔线跟三档分开 */
.global-col {
  background: #fbfbfd;
  box-shadow: inset -1px 0 0 var(--line);
}

/* 底栏在滚动区之外，永远可见 */
.foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 18px;
  background: var(--bg);
  border-top: 1px solid var(--line);
}
</style>
