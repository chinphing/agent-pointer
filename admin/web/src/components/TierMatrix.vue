<script setup lang="ts">
/**
 * 档位映射矩阵（设计图 08/09）。
 *
 * 配的是「场景 × 档位 → 模型」，不是「员工用哪一档」——选哪一档仍由员工决定。
 * 下发形状见设计文档 §7.2：`{agentModeLlm:{场景:{fast|standard|expert:{providerId,model}}}, mediaModeLlm:{...}}`。
 * 留空 = 继承默认（客户端用自己内置的档位模型）。
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

const SCENES = [
  { key: 'general', label: '通用对话', group: 'agentModeLlm' },
  { key: 'coder', label: '编码', group: 'agentModeLlm' },
  { key: 'computer', label: '桌面操作', group: 'agentModeLlm' },
  { key: 'web_search', label: '联网搜索', group: 'agentModeLlm' },
  { key: 'image', label: '图片理解', group: 'mediaModeLlm' },
  { key: 'audio', label: '音频转写', group: 'mediaModeLlm' },
  { key: 'video', label: '视频理解', group: 'mediaModeLlm' },
]
/** 与客户端 `thinkingIntensity` 阶梯一致；留空 = 继承模型默认。 */
const THINKING = [
  { key: '', label: '强度·继承' },
  { key: 'off', label: '强度·关' },
  { key: 'low', label: '强度·低' },
  { key: 'medium', label: '强度·中' },
  { key: 'high', label: '强度·高' },
  { key: 'max', label: '强度·最高' },
]
const TIERS = [
  { key: 'fast', label: '快速' },
  { key: 'standard', label: '标准' },
  { key: 'expert', label: '高级' },
]

const payload = ref<any>({})
const error = ref('')
const notice = ref('')
const busy = ref(false)

watch(
  () => props.resource,
  (r) => {
    // 深拷贝：直接改 props 里的对象会污染列表数据。
    payload.value = r ? JSON.parse(JSON.stringify(r.payload ?? {})) : {}
    error.value = ''
  },
  { immediate: true }
)

/** providerId 用服务商标识（payload 里就是这么写的）。 */
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

function setIntensity(scene: string, group: string, tier: string, value: string) {
  const cur = payload.value?.[group]?.[scene]?.[tier]
  if (!cur) return
  if (value) cur.thinkingIntensity = value
  else delete cur.thinkingIntensity
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

async function save() {
  if (!props.resource) return
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    await api.put(`/api/admin/resources/${props.resource.id}`, { payload: payload.value })
    notice.value = '档位映射已保存'
    emit('saved')
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
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
    notice.value = '已创建默认档位映射'
    emit('saved')
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="panel" style="margin-top: 16px">
    <div class="spread" style="margin-bottom: 10px">
      <div>
        <h2>档位映射</h2>
        <div class="small muted" style="margin-top: 2px">
          场景 × 档位 → 模型；强度留空则继承模型默认。
        </div>
      </div>
      <button v-if="canWrite && resource" class="primary" :disabled="busy" @click="save">
        保存档位映射
      </button>
    </div>

    <div v-if="error" class="err">{{ error }}</div>
    <div v-if="notice" class="okmsg">{{ notice }}</div>

    <div v-if="!resource" class="empty">
      还没有档位映射。
      <button v-if="canWrite" class="tiny" style="margin-left: 8px" :disabled="busy" @click="createDefault">
        创建默认档位映射
      </button>
    </div>

    <template v-else>
      <p class="small muted" style="margin: 0 0 12px">
        每格 = 该场景该档位使用的模型，留空表示「继承默认」（客户端用自己的内置档位模型）。
        这是<strong>默认值</strong>：员工自己改过某一档，这里的配置不覆盖他。
      </p>
      <table>
        <thead>
          <tr>
            <th style="width: 160px">场景</th>
            <th v-for="t in TIERS" :key="t.key">{{ t.label }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="s in SCENES" :key="s.group + s.key">
            <td>
              {{ s.label }}
              <span class="small muted mono">{{ s.key }}</span>
            </td>
            <td v-for="t in TIERS" :key="t.key" class="tier-cell">
              <select
                :value="modelOf(s.key, s.group, t.key)"
                :disabled="!canWrite"
                @change="setModel(s.key, s.group, t.key, ($event.target as HTMLSelectElement).value)"
              >
                <option value="">继承默认</option>
                <option v-for="m in modelOptions" :key="m.slug" :value="m.slug">{{ m.label }}</option>
              </select>
              <select
                class="tiny intensity"
                :value="intensityOf(s.key, s.group, t.key)"
                :disabled="!canWrite || !modelOf(s.key, s.group, t.key)"
                :title="modelOf(s.key, s.group, t.key) ? '思考强度' : '先选模型再配强度'"
                @change="setIntensity(s.key, s.group, t.key, ($event.target as HTMLSelectElement).value)"
              >
                <option v-for="o in THINKING" :key="o.key" :value="o.key">{{ o.label }}</option>
              </select>
            </td>
          </tr>
        </tbody>
      </table>
    </template>
  </div>
</template>
