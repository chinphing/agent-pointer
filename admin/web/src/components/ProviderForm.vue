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
import { api, errText } from '../api'

const props = defineProps<{ resource: any | null }>()
const emit = defineEmits<{ (e: 'saved'): void; (e: 'cancel'): void }>()

const TEMPLATES = [
  {
    id: 'bailian',
    label: '阿里云百炼（DashScope）',
    slug: 'bailian',
    name: '阿里云百炼',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  },
  {
    id: 'deepseek',
    label: 'DeepSeek',
    slug: 'deepseek',
    name: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com/v1',
  },
  {
    id: 'openai',
    label: 'OpenAI',
    slug: 'openai',
    name: 'OpenAI',
    baseUrl: 'https://api.openai.com/v1',
  },
  { id: 'custom', label: '其他（OpenAI 兼容）', slug: '', name: '', baseUrl: '' },
]

const isEdit = computed(() => Boolean(props.resource))
const template = ref('bailian')
const form = ref({ slug: '', name: '', baseUrl: '', apiKey: '' })
const busy = ref(false)
const error = ref('')
const notice = ref('')

const testing = ref(false)
const testResult = ref<{ ok: boolean; message: string } | null>(null)

/** 这个服务商下要保留的模型（新建时=要建的，编辑时=保存后仍存在的）。 */
const modelList = ref<string[]>([])
/** 编辑模式：slug → 已存在的模型资源 id，用于判断哪些是新增、哪些要删。 */
const existingIds = ref<Record<string, string>>({})
/** 编辑模式：本地已有的模型（用于和远端拉回来的做差集）。 */
const loaded = ref(false)

const newModel = ref('')

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
    apiKey: '',
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
  notice.value = ''
  try {
    const r = await api.post('/api/admin/providers/test', {
      baseUrl: form.value.baseUrl.trim(),
      apiKey: form.value.apiKey.trim(),
    })
    testResult.value = { ok: Boolean(r.ok), message: r.message || (r.ok ? '连接正常' : '连接失败') }
    if (r.ok) {
      const remote: string[] = r.models || []
      const added = remote.filter((m) => !modelList.value.includes(m))
      if (remote.length === 0) {
        notice.value = '连接正常，但这个接口没有返回模型清单——可以手动添加'
      } else if (added.length === 0) {
        notice.value = `远端 ${remote.length} 个模型都已在列表里`
      } else {
        modelList.value = [...modelList.value, ...added]
        notice.value = `拉取到 ${remote.length} 个模型，已加入 ${added.length} 个（不需要的可以删掉）`
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
  notice.value = ''
  if (!form.value.slug.trim()) return (error.value = '请填写标识')
  if (!form.value.name.trim()) return (error.value = '请填写名称')
  if (!form.value.baseUrl.trim()) return (error.value = '请填写接口地址')

  busy.value = true
  try {
    const payload = { baseUrl: form.value.baseUrl.trim() }

    if (isEdit.value) {
      const removed = Object.keys(existingIds.value).filter((s) => !modelList.value.includes(s))
      if (removed.length) {
        const used = await modelsReferencedByTiers(removed)
        if (used.length) {
          error.value = `这些模型还被档位映射引用，先改档位再删：${used.join('、')}`
          return
        }
      }

      await api.put(`/api/admin/resources/${props.resource.id}`, {
        name: form.value.name.trim(),
        payload,
        ...(form.value.apiKey.trim() ? { secret: form.value.apiKey.trim() } : {}),
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
    await api.put(`/api/admin/resources/${created.id}/grants`, { items: [{ type: 'all' }] })
    emit('saved')
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
      <h2>{{ isEdit ? '编辑模型服务' : '新建模型服务' }}</h2>

      <template v-if="!isEdit">
        <label>选择服务商</label>
        <div class="tpl-grid">
          <button
            v-for="t in TEMPLATES"
            :key="t.id"
            type="button"
            class="tpl"
            :class="{ active: template === t.id }"
            @click="applyTemplate(t.id)"
          >
            {{ t.label }}
          </button>
        </div>
      </template>

      <div class="two">
        <div>
          <label>名称</label>
          <input v-model="form.name" placeholder="阿里云百炼" />
        </div>
        <div>
          <label>标识（建后不可改）</label>
          <input v-model="form.slug" :disabled="isEdit" placeholder="bailian" />
        </div>
      </div>

      <label>接口地址</label>
      <input v-model="form.baseUrl" class="mono" placeholder="https://…/v1" />
      <p class="small muted" style="margin: 4px 0 0">
        填到 <span class="mono">/v1</span> 为止，系统会在后面拼 <span class="mono">/models</span> 探测模型。
      </p>

      <label>API Key{{ isEdit ? '（留空表示不修改）' : '' }}</label>
      <input v-model="form.apiKey" type="password" placeholder="sk-…" />

      <div class="row" style="gap: 10px; margin-top: 12px">
        <button class="tiny" :disabled="testing" @click="testConnection">
          {{ testing ? '探测中…' : '测试连接并获取模型' }}
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
            模型（{{ modelList.length }} 个）
            <span v-if="isEdit" class="muted">· 删掉的行保存后才会真正移除</span>
          </span>
        </div>

        <div class="row" style="gap: 8px; margin-top: 8px">
          <input
            v-model="newModel"
            class="tiny"
            style="flex: 1"
            placeholder="手动添加模型名，回车确认"
            @keyup.enter="addModel"
          />
          <button class="tiny" :disabled="!newModel.trim()" @click="addModel">添加</button>
        </div>

        <div v-if="!modelList.length && loaded" class="small muted" style="padding: 6px 2px">
          还没有模型。点上面的「测试连接并获取模型」自动拉，或在下面手动加。
        </div>

        <div class="model-list">
          <div v-for="m in modelList" :key="m" class="model-row">
            <span class="mono">{{ m }}</span>
            <button class="tiny" title="从列表移除" @click="removeModel(m)">移除</button>
          </div>
        </div>

      </div>

      <div v-if="error" class="err" style="margin-top: 12px">{{ error }}</div>
      <div v-if="notice" class="okmsg" style="margin-top: 12px">{{ notice }}</div>

      <div class="modal-actions">
        <button @click="emit('cancel')">取消</button>
        <button class="primary" :disabled="busy" @click="submit">
          {{ isEdit ? '保存' : modelList.length ? `创建并启用 ${modelList.length} 个模型` : '创建' }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tpl-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 8px; margin-top: 6px; }
.tpl { text-align: left; font-size: 13px; }
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
</style>
