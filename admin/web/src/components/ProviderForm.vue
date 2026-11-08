<script setup lang="ts">
/**
 * 模型服务的新建 / 编辑表单。
 *
 * 之前是「填 slug、填名称、手敲 payload JSON、填密钥」——对配服务商的人来说
 * 等于让他记接口地址和 JSON 结构。这里改成向导式：
 * 选模板 → 自动带出地址 → 填密钥 → 测试连接（顺带拉回模型清单）→ 勾要启用的模型 → 保存。
 *
 * 交互参考客户端设置里的模型服务：模板驱动 + 拉取模型，而不是手填。
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
const form = ref({
  slug: '',
  name: '',
  baseUrl: '',
  apiKey: '',
})
const busy = ref(false)
const error = ref('')
const notice = ref('')

const testing = ref(false)
const testResult = ref<{ ok: boolean; message: string } | null>(null)
const fetched = ref<string[]>([])
const picked = ref<string[]>([])
const search = ref('')

const filteredModels = computed(() => {
  const q = search.value.trim().toLowerCase()
  return q ? fetched.value.filter((m) => m.toLowerCase().includes(q)) : fetched.value
})

// 编辑已有服务商时用它的现值初始化
if (props.resource) {
  const payload = props.resource.payload || {}
  form.value = {
    slug: props.resource.slug || '',
    name: props.resource.name || '',
    baseUrl: payload.baseUrl || '',
    apiKey: '',
  }
} else {
  applyTemplate('bailian')
}

// 上一次由模板自动填进去的值：只有用户没动过才跟着模板换，
// 否则从「百炼」切到「DeepSeek」会留下一个叫「阿里云百炼」的 DeepSeek 服务商。
const autoFilled = ref({ slug: '', name: '' })

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
  // 换模板后旧的探测结果不再可信
  testResult.value = null
  fetched.value = []
  picked.value = []
}

async function testConnection() {
  testing.value = true
  testResult.value = null
  try {
    const r = await api.post('/api/admin/providers/test', {
      baseUrl: form.value.baseUrl.trim(),
      apiKey: form.value.apiKey.trim(),
    })
    testResult.value = { ok: Boolean(r.ok), message: r.message || (r.ok ? '连接正常' : '连接失败') }
    if (r.ok) {
      fetched.value = r.models || []
      if (fetched.value.length) {
        notice.value = `拉取到 ${fetched.value.length} 个模型，勾选要启用的`
      } else {
        notice.value = '连接正常，但这个接口没有返回模型清单——保存后可手动加模型'
      }
    }
  } catch (e) {
    testResult.value = { ok: false, message: errText(e) }
  } finally {
    testing.value = false
  }
}

function togglePick(id: string) {
  const i = picked.value.indexOf(id)
  if (i >= 0) picked.value.splice(i, 1)
  else picked.value.push(id)
}

function pickAll() {
  picked.value = filteredModels.value.slice()
}

function pickNone() {
  picked.value = []
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
      await api.put(`/api/admin/resources/${props.resource.id}`, {
        name: form.value.name.trim(),
        payload,
        ...(form.value.apiKey.trim() ? { secret: form.value.apiKey.trim() } : {}),
      })
      notice.value = '已保存'
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
    // 勾选的模型一并建出来，省得再逐个手敲
    for (const model of picked.value) {
      await api.post('/api/admin/resources', {
        kind: 'model',
        slug: model,
        name: model,
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
    <div class="modal" style="width: 620px">
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

      <div v-if="fetched.length" class="models">
        <div class="spread" style="margin-bottom: 8px">
          <span class="small">选择要启用的模型（{{ picked.length }} / {{ fetched.length }}）</span>
          <span class="row" style="gap: 8px">
            <button class="tiny" @click="pickAll">全选</button>
            <button class="tiny" @click="pickNone">清空</button>
          </span>
        </div>
        <input v-model="search" class="tiny" style="width: 100%; margin-bottom: 8px" placeholder="搜索模型名" />
        <div class="model-list">
          <label v-for="m in filteredModels" :key="m" class="model-item">
            <input
              type="checkbox"
              style="width: auto"
              :checked="picked.includes(m)"
              @change="togglePick(m)"
            />
            <span class="mono">{{ m }}</span>
          </label>
        </div>
      </div>

      <div v-if="error" class="err" style="margin-top: 12px">{{ error }}</div>
      <div v-if="notice" class="okmsg" style="margin-top: 12px">{{ notice }}</div>

      <div class="modal-actions">
        <button @click="emit('cancel')">取消</button>
        <button class="primary" :disabled="busy" @click="submit">
          {{ isEdit ? '保存' : picked.length ? `创建并启用 ${picked.length} 个模型` : '创建' }}
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
.models { margin-top: 14px; border: 1px solid var(--line); border-radius: var(--radius-sm); padding: 12px; }
.model-list { max-height: 200px; overflow: auto; display: flex; flex-direction: column; gap: 2px; }
.model-item { display: flex; align-items: center; gap: 8px; margin: 0; padding: 5px 6px; border-radius: 6px; color: var(--fg); }
.model-item:hover { background: var(--panel-alt); }
</style>
