<script setup lang="ts">
/**
 * 档位映射矩阵（设计图 08/09）。
 *
 * 配的是「场景 × 档位 → 模型」，不是「员工用哪一档」——选哪一档仍由员工决定。
 * 下发形状见设计文档 §7.2：`{agentModeLlm:{场景:{fast|standard|expert:{providerId,model}}}, mediaModeLlm:{...}}`。
 *
 * 这是**配置矩阵**，不是表单：一眼要能看出哪些格子被改过、哪些还在继承。
 * 所以每格默认是「文字 + 幽灵下拉」，未配置显示灰色「继承默认」，配置了才显示模型名；
 * 保存放在底部、有改动才点亮。
 */
import { computed, ref, watch } from 'vue'
import { api, errText } from '../api'

const props = defineProps<{
  resource: any | null
  models: any[]
  providers: any[]
  canWrite: boolean
}>()
const emit = defineEmits<{ (e: 'saved'): void }>()

/** 按用途分组：对话/工具走 agentModeLlm，媒体走 mediaModeLlm。 */
const GROUPS = [
  {
    label: '对话与工具',
    scenes: [
      { key: 'general', label: '通用对话', group: 'agentModeLlm' },
      { key: 'coder', label: '编码', group: 'agentModeLlm' },
      { key: 'computer', label: '桌面操作', group: 'agentModeLlm' },
      { key: 'web_search', label: '联网搜索', group: 'agentModeLlm' },
    ],
  },
  {
    label: '媒体',
    scenes: [
      { key: 'image', label: '图片理解', group: 'mediaModeLlm' },
      { key: 'audio', label: '音频转写', group: 'mediaModeLlm' },
      { key: 'video', label: '视频理解', group: 'mediaModeLlm' },
    ],
  },
]

const TIERS = [
  { key: 'fast', label: '快速' },
  { key: 'standard', label: '标准' },
  { key: 'expert', label: '高级' },
]

/** 与客户端 `thinkingIntensity` 阶梯一致；留空 = 继承模型默认。 */
const THINKING = [
  { key: '', label: '—' },
  { key: 'off', label: '关' },
  { key: 'low', label: '低' },
  { key: 'medium', label: '中' },
  { key: 'high', label: '高' },
  { key: 'max', label: '最高' },
]

const payload = ref<any>({})
const saved = ref('{}')
const error = ref('')
const notice = ref('')
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

const modelOptions = computed(() =>
  (props.models || []).map((m: any) => ({
    slug: m.slug,
    label: m.name || m.slug,
    providerId: providerSlugById.value[m.parentId] || '',
  }))
)

function cell(scene: string, group: string, tier: string): any {
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
  const cur = payload.value?.[group]?.[scene]?.[tier]
  if (!cur) return
  if (value) cur.thinkingIntensity = value
  else delete cur.thinkingIntensity
}

function clearCell(scene: string, group: string, tier: string) {
  setModel(scene, group, tier, '')
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
  notice.value = ''
  try {
    await api.put(`/api/admin/resources/${props.resource.id}`, { payload: payload.value })
    saved.value = stable(payload.value)
    notice.value = '已保存'
    emit('saved')
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

function revert() {
  payload.value = JSON.parse(saved.value)
  notice.value = ''
  error.value = ''
}

async function createDefault() {
  busy.value = true
  error.value = ''
  try {
    await api.post('/api/admin/resources', {
      kind: 'tier_profile',
      slug: 'default',
      name: '默认档位映射',
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
      还没有档位映射。
      <button v-if="canWrite" class="tiny" style="margin-left: 8px" :disabled="busy" @click="createDefault">
        创建默认档位映射
      </button>
    </div>

    <template v-else>
      <p class="hint">
        每格选该场景该档位用的模型；留空就是「继承默认」，客户端用自己的内置模型。
        选了模型后可再调<strong>思考强度</strong>（关 / 低 / 中 / 高 / 最高），不选就跟随模型默认。
        管理员配的只是默认值，员工自己改过的那一档不会被覆盖。
      </p>

      <div class="matrix-scroll">
      <div class="matrix">
        <div class="head-row">
          <div class="scene-col">场景</div>
          <div v-for="t in TIERS" :key="t.key" class="tier-head">{{ t.label }}</div>
        </div>

        <template v-for="g in GROUPS" :key="g.label">
          <div class="group-row">{{ g.label }}</div>
          <div v-for="s in g.scenes" :key="s.group + s.key" class="scene-row">
            <div class="scene-col" :title="s.key">{{ s.label }}</div>
            <div
              v-for="t in TIERS"
              :key="t.key"
              class="cell"
              :class="{ filled: !!modelOf(s.key, s.group, t.key) }"
            >
              <select
                class="ghost"
                :class="{ filled: !!modelOf(s.key, s.group, t.key) }"
                :value="modelOf(s.key, s.group, t.key)"
                :disabled="!canWrite"
                @change="setModel(s.key, s.group, t.key, ($event.target as HTMLSelectElement).value)"
              >
                <option value="">继承默认</option>
                <option v-for="m in modelOptions" :key="m.slug" :value="m.slug">{{ m.label }}</option>
              </select>
              <select
                v-if="modelOf(s.key, s.group, t.key)"
                class="mini"
                :value="intensityOf(s.key, s.group, t.key)"
                :disabled="!canWrite"
                title="思考强度"
                @change="setIntensity(s.key, s.group, t.key, ($event.target as HTMLSelectElement).value)"
              >
                <option v-for="o in THINKING" :key="o.key" :value="o.key">{{ o.label }}</option>
              </select>
              <button
                v-if="modelOf(s.key, s.group, t.key) && canWrite"
                class="clear"
                title="清空这一格"
                @click="clearCell(s.key, s.group, t.key)"
              >×</button>
            </div>
          </div>
        </template>
      </div>
      </div>

      <div class="foot">
        <span class="small muted">
          已配置 {{ filledCount }} / {{ totalCells }} 格
          <template v-if="dirty"> · 有未保存的改动</template>
        </span>
        <div v-if="canWrite" class="row" style="gap: 8px">
          <button v-if="dirty" class="tiny" :disabled="busy" @click="revert">撤销改动</button>
          <button class="primary" :disabled="busy || !dirty" @click="save">保存</button>
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
  flex: 0 0 62px;
  width: 62px;
  padding: 3px 4px;
  font-size: 12px;
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
