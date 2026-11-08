<script setup lang="ts">
/**
 * 资源详情抽屉（设计图 05）。
 *
 * 右侧滑出，把「状态 / 可见范围 / 版本历史 / 已授权」集中到一处；
 * 行内按钮仍然保留，抽屉里的操作按钮只是复用同一批弹窗（emit action）。
 */
import { computed, ref, watch } from 'vue'
import { api, errText } from '../api'
import { humanSize, when } from '../lib/format'

const props = defineProps<{ resource: any; canWrite: boolean }>()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'action', name: 'edit' | 'grants' | 'versions' | 'toggle'): void
  (e: 'refresh'): void
}>()

const grants = ref<any[]>([])
const versions = ref<any[]>([])
const error = ref('')
const loading = ref(false)

const KIND_LABEL: Record<string, string> = {
  provider: '模型服务',
  model: '模型',
  skill: '技能',
  plugin: '插件',
  mcp: 'MCP',
  tier_profile: '档位映射',
}

const kindLabel = computed(() => KIND_LABEL[props.resource?.kind] || props.resource?.kind || '')

const isAll = computed(() => grants.value.some((g) => g.type === 'all'))

async function load() {
  const r = props.resource
  if (!r) return
  loading.value = true
  error.value = ''
  grants.value = []
  versions.value = []
  try {
    const g = await api.get(`/api/admin/resources/${r.id}/grants`)
    grants.value = g.items || []
  } catch (e) {
    error.value = errText(e)
  }
  if (r.kind === 'skill' || r.kind === 'plugin') {
    try {
      const v = await api.get(`/api/admin/resources/${r.id}/versions`)
      versions.value = v.items || []
    } catch {
      /* 没有版本时不算错误 */
    }
  }
  loading.value = false
}

watch(() => props.resource?.id, load, { immediate: true })

function grantLabel(g: any) {
  if (g.type === 'all') return '全员'
  return g.type === 'role' ? `角色 · ${g.id.slice(0, 8)}` : `成员 · ${g.id.slice(0, 8)}`
}
</script>

<template>
  <div class="drawer-mask" @click.self="emit('close')">
    <aside class="drawer">
      <div class="row" style="justify-content: space-between; align-items: flex-start">
        <div>
          <h2 style="margin: 0">{{ resource.name }}</h2>
          <div class="small muted" style="margin-top: 4px">
            {{ kindLabel }} · {{ resource.slug }} · {{ when(resource.updatedAt) }} 更新
          </div>
        </div>
        <button class="tiny" @click="emit('close')">×</button>
      </div>

      <div v-if="error" class="err" style="margin-top: 12px">{{ error }}</div>

      <section style="margin-top: 18px">
        <div class="label">状态</div>
        <span class="tag" :class="resource.enabled ? 'ok' : 'plain'">
          {{ resource.enabled ? '已启用' : '已停用' }}
        </span>
      </section>

      <section style="margin-top: 16px">
        <div class="label">可见范围</div>
        <div v-if="loading" class="small muted">加载中…</div>
        <div v-else-if="isAll" class="row" style="gap: 6px; flex-wrap: wrap">
          <span class="tag ok">全员</span>
        </div>
        <div v-else-if="grants.length" class="row" style="gap: 6px; flex-wrap: wrap">
          <span v-for="g in grants" :key="g.type + g.id" class="tag">{{ grantLabel(g) }}</span>
        </div>
        <div v-else class="small muted">未授权 —— 员工端看不到这个资源</div>
      </section>

      <section v-if="versions.length" style="margin-top: 16px">
        <div class="label">版本历史</div>
        <table style="margin-top: 6px">
          <thead>
            <tr><th>版本</th><th>大小</th><th>时间</th></tr>
          </thead>
          <tbody>
            <tr v-for="v in versions" :key="v.id">
              <td class="mono">
                {{ v.version }}
                <span v-if="v.isCurrent" class="tag ok" style="margin-left: 4px">当前</span>
              </td>
              <td class="small muted">{{ humanSize(v.sizeBytes) }}</td>
              <td class="small muted">{{ when(v.createdAt) }}</td>
            </tr>
          </tbody>
        </table>
      </section>

      <p class="small muted" style="margin-top: 18px">
        停用后成员端下一轮同步即不可见；已下载的副本不会删除。
      </p>

      <div v-if="canWrite" class="row" style="gap: 8px; margin-top: 10px; flex-wrap: wrap">
        <button class="tiny" @click="emit('action', 'edit')">编辑</button>
        <button class="tiny" @click="emit('action', 'grants')">授权</button>
        <button v-if="resource.kind === 'skill' || resource.kind === 'plugin'" class="tiny" @click="emit('action', 'versions')">
          版本
        </button>
        <button class="tiny" :class="{ danger: resource.enabled }" @click="emit('action', 'toggle')">
          {{ resource.enabled ? '停用资源' : '启用资源' }}
        </button>
      </div>
    </aside>
  </div>
</template>

<style scoped>
.drawer-mask {
  position: fixed;
  inset: 0;
  background: rgba(24, 24, 27, 0.28);
  display: flex;
  justify-content: flex-end;
  z-index: 40;
}
.drawer {
  width: 420px;
  max-width: 92vw;
  height: 100%;
  overflow: auto;
  background: var(--panel);
  border-left: 1px solid var(--line);
  padding: 22px 24px 40px;
}
.label {
  font-size: 12px;
  color: var(--muted);
  margin-bottom: 6px;
}
</style>
