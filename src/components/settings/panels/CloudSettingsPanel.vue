<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { Cloud, Loader2, RefreshCw } from 'lucide-vue-next'
import { usePlatformAuthStore } from '../../../stores/platformAuth'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import {
  agentPhaseLabel,
  agentUiPhase,
  canOpenCloudAgent,
  closeCloudAgent,
  focusCloudAgent,
  formatApiError,
  getCloudPlatformMe,
  getCloudShopPricing,
  isCloudAgentWindowOpen,
  listCloudAgents,
  listCloudShopRegions,
  openCloudAgent,
  previewCloudShop,
  purchaseCloudAgent,
  releaseCloudAgent,
  renewCloudAgent,
  type AgentStatusFilter,
  type CloudAgent,
  type PackageKind,
  type PlatformMe,
  type ShopPreviewRequest,
  type ShopPreviewResult,
  type ShopPricing,
  type ShopRegion
} from '../../../lib/cloudAgents'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const platformAuth = usePlatformAuthStore()
const { loginPlatformAccount } = props.form

const loading = ref(false)
const error = ref<string | null>(null)
const me = ref<PlatformMe | null>(null)
const regions = ref<ShopRegion[]>([])
const agents = ref<CloudAgent[]>([])
const statusFilter = ref<AgentStatusFilter>('running')
const openWindowMap = ref<Record<string, boolean>>({})

const purchaseOpen = ref(false)
const purchaseTab = ref<PackageKind>('hourly_trial')
const purchaseRegionId = ref('')
const purchaseHours = ref('1')
const purchaseMonthlyQty = ref('1')
const purchaseYearlyQty = ref('1')
const purchaseTierId = ref('')
const purchasePricing = ref<ShopPricing | null>(null)
const purchasePreview = ref<ShopPreviewResult | null>(null)
const purchaseSubmitting = ref(false)

const renewAgent = ref<CloudAgent | null>(null)
const renewPreview = ref<ShopPreviewResult | null>(null)
const renewSubmitting = ref(false)

const releaseAgent = ref<CloudAgent | null>(null)
const releaseSubmitting = ref(false)

const openingAgentId = ref<string | null>(null)

const loggedIn = computed(() => platformAuth.session.logged_in)

async function refreshOpenWindowStates(list: CloudAgent[]) {
  const next: Record<string, boolean> = {}
  await Promise.all(
    list.map(async a => {
      try {
        next[a.id] = await isCloudAgentWindowOpen(a.id)
      } catch {
        next[a.id] = false
      }
    })
  )
  openWindowMap.value = next
}

async function loadAll() {
  if (!loggedIn.value) return
  loading.value = true
  error.value = null
  try {
    const [meRow, regionRows, agentPage] = await Promise.all([
      getCloudPlatformMe(),
      listCloudShopRegions(),
      listCloudAgents(1, 20, statusFilter.value)
    ])
    me.value = meRow
    regions.value = regionRows
    agents.value = agentPage.items
    if (!purchaseRegionId.value && regionRows.length > 0) {
      purchaseRegionId.value = regionRows[0].id
    }
    await refreshOpenWindowStates(agentPage.items)
  } catch (e) {
    error.value = formatApiError(e)
  } finally {
    loading.value = false
  }
}

async function reloadAgents() {
  if (!loggedIn.value) return
  try {
    const page = await listCloudAgents(1, 20, statusFilter.value)
    agents.value = page.items
    await refreshOpenWindowStates(page.items)
  } catch (e) {
    error.value = formatApiError(e)
  }
}

function buildPreviewBody(
  regionId: string,
  kind: PackageKind,
  hours: string,
  monthlyQty: string,
  yearlyQty: string,
  tierId: string
): ShopPreviewRequest | null {
  const body: ShopPreviewRequest = {
    region_id: regionId,
    package_kind: kind,
    machine_tier_id: tierId.trim() || null
  }
  if (kind === 'hourly_trial' || kind === 'hourly_spot') {
    const h = parseInt(hours, 10)
    if (!Number.isFinite(h) || h < 1) return null
    body.hours = h
  } else if (kind === 'monthly') {
    const q = parseInt(monthlyQty, 10)
    if (!Number.isFinite(q) || q < 1) return null
    body.quantity = q
  } else {
    const q = parseInt(yearlyQty, 10)
    if (!Number.isFinite(q) || q < 1) return null
    body.quantity = q
  }
  return body
}

async function loadPurchasePricing() {
  if (!purchaseRegionId.value) return
  try {
    purchasePricing.value = await getCloudShopPricing(purchaseRegionId.value)
    const tiers = purchasePricing.value.machine_tiers ?? []
    if (!purchaseTierId.value && tiers.length > 0) {
      purchaseTierId.value = tiers.find(t => t.is_default)?.id ?? tiers[0].id
    }
  } catch (e) {
    error.value = formatApiError(e)
  }
}

async function runPurchasePreview() {
  const body = buildPreviewBody(
    purchaseRegionId.value,
    purchaseTab.value,
    purchaseHours.value,
    purchaseMonthlyQty.value,
    purchaseYearlyQty.value,
    purchaseTierId.value
  )
  if (!body) {
    error.value = '请填写有效的购买时长'
    return
  }
  try {
    purchasePreview.value = await previewCloudShop(body)
  } catch (e) {
    error.value = formatApiError(e)
  }
}

async function confirmPurchase() {
  const body = buildPreviewBody(
    purchaseRegionId.value,
    purchaseTab.value,
    purchaseHours.value,
    purchaseMonthlyQty.value,
    purchaseYearlyQty.value,
    purchaseTierId.value
  )
  if (!body) return
  purchaseSubmitting.value = true
  error.value = null
  try {
    const result = await purchaseCloudAgent(body)
    me.value = { ...me.value!, balance_yuan: result.balance_yuan }
    purchaseOpen.value = false
    statusFilter.value = 'running'
    await reloadAgents()
  } catch (e) {
    error.value = formatApiError(e)
  } finally {
    purchaseSubmitting.value = false
  }
}

async function onOpenAgent(agent: CloudAgent) {
  if (!canOpenCloudAgent(agent)) return
  openingAgentId.value = agent.id
  error.value = null
  try {
    await openCloudAgent(agent.id)
    openWindowMap.value = { ...openWindowMap.value, [agent.id]: true }
  } catch (e) {
    error.value = formatApiError(e)
  } finally {
    openingAgentId.value = null
  }
}

async function onFocusAgent(agentId: string) {
  try {
    await focusCloudAgent(agentId)
  } catch (e) {
    error.value = formatApiError(e)
  }
}

async function onCloseWindow(agentId: string) {
  try {
    await closeCloudAgent(agentId)
    openWindowMap.value = { ...openWindowMap.value, [agentId]: false }
  } catch (e) {
    error.value = formatApiError(e)
  }
}

function startRenew(agent: CloudAgent) {
  renewAgent.value = agent
  renewPreview.value = null
}

async function runRenewPreview() {
  const agent = renewAgent.value
  if (!agent?.package_kind || !agent.server_region?.id) return
  const kind = agent.package_kind as PackageKind
  const body = buildPreviewBody(
    agent.server_region.id,
    kind,
    String(agent.hours_purchased ?? 1),
    String(agent.prepaid_months ?? 1),
    String(agent.prepaid_years ?? 1),
    agent.machine_tier_id ?? ''
  )
  if (!body) return
  try {
    renewPreview.value = await previewCloudShop(body)
  } catch (e) {
    error.value = formatApiError(e)
  }
}

async function confirmRenew() {
  const agent = renewAgent.value
  if (!agent?.package_kind || !agent.server_region?.id) return
  const kind = agent.package_kind as PackageKind
  const body = buildPreviewBody(
    agent.server_region.id,
    kind,
    String(agent.hours_purchased ?? 1),
    String(agent.prepaid_months ?? 1),
    String(agent.prepaid_years ?? 1),
    agent.machine_tier_id ?? ''
  )
  if (!body) return
  renewSubmitting.value = true
  try {
    const result = await renewCloudAgent(agent.id, body)
    me.value = { ...me.value!, balance_yuan: result.balance_yuan }
    renewAgent.value = null
    await reloadAgents()
  } catch (e) {
    error.value = formatApiError(e)
  } finally {
    renewSubmitting.value = false
  }
}

async function confirmRelease() {
  const agent = releaseAgent.value
  if (!agent) return
  releaseSubmitting.value = true
  try {
    await releaseCloudAgent(agent.id)
    releaseAgent.value = null
    await reloadAgents()
  } catch (e) {
    error.value = formatApiError(e)
  } finally {
    releaseSubmitting.value = false
  }
}

watch(statusFilter, () => {
  void reloadAgents()
})

watch(purchaseRegionId, () => {
  if (purchaseOpen.value) void loadPurchasePricing()
})

watch(loggedIn, v => {
  if (v) void loadAll()
})

onMounted(() => {
  if (loggedIn.value) void loadAll()
})
</script>

<template>
  <div class="space-y-5">
    <div>
      <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
        <Cloud class="w-4 h-4 text-accent" />云主机
      </h3>
      <p class="mt-0.5 text-xs text-muted">购买、续费与管理远程 Pointer 实例</p>
    </div>

    <div v-if="!loggedIn" class="rounded-xl border border-border panel p-5 space-y-3">
      <p class="text-sm text-muted">登录平台账户后可管理云主机</p>
      <button
        type="button"
        class="h-8 px-4 rounded-lg bg-accent text-sm font-medium text-white hover:opacity-95 cursor-pointer"
        @click="loginPlatformAccount"
      >
        浏览器登录
      </button>
    </div>

    <template v-else>
      <div
        v-if="error"
        class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger"
        role="alert"
      >
        {{ error }}
      </div>

      <div class="flex flex-wrap items-center justify-between gap-3">
        <div v-if="me" class="text-sm">
          <span class="text-muted">余额 </span>
          <span class="font-semibold tabular-nums text-accent">{{ me.balance_yuan }}</span>
          <span class="text-muted text-xs"> 元</span>
        </div>
        <div class="flex flex-wrap gap-2">
          <button
            type="button"
            class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover cursor-pointer inline-flex items-center gap-1"
            :disabled="loading"
            @click="loadAll"
          >
            <RefreshCw class="w-3.5 h-3.5" />刷新
          </button>
          <button
            type="button"
            class="h-8 px-3 rounded-lg bg-accent text-xs font-medium text-white hover:opacity-95 cursor-pointer"
            @click="purchaseOpen = true"
          >
            购买云主机
          </button>
        </div>
      </div>

      <div class="flex flex-wrap gap-2">
        <button
          v-for="f in ([
            ['running', '运行中'],
            ['starting', '启动中'],
            ['released', '已释放'],
            ['all', '全部']
          ] as const)"
          :key="f[0]"
          type="button"
          class="h-7 px-3 rounded-full text-xs border cursor-pointer transition-colors"
          :class="
            statusFilter === f[0]
              ? 'border-accent/40 bg-accent/10 text-accent'
              : 'border-border text-muted hover:bg-hover'
          "
          @click="statusFilter = f[0]"
        >
          {{ f[1] }}
        </button>
      </div>

      <div v-if="loading && agents.length === 0" class="py-8 flex justify-center text-muted">
        <Loader2 class="w-5 h-5 animate-spin" />
      </div>

      <div v-else-if="agents.length === 0" class="rounded-xl border border-border panel p-6 text-center text-sm text-muted">
        暂无实例
      </div>

      <div v-else class="space-y-3">
        <div
          v-for="agent in agents"
          :key="agent.id"
          class="rounded-xl border border-border panel p-4 space-y-3"
        >
          <div class="flex flex-wrap items-start justify-between gap-2">
            <div>
              <p class="text-sm font-medium text-foreground">
                {{ agent.server_region?.name_zh || '云主机' }}
              </p>
              <p class="text-xs text-muted mt-0.5">
                到期 {{ agent.expires_at ? new Date(agent.expires_at).toLocaleString() : '—' }}
              </p>
            </div>
            <span
              class="shrink-0 rounded-md px-2 py-0.5 text-[11px] border"
              :class="
                agentUiPhase(agent) === 'running'
                  ? 'border-success/30 bg-success/10 text-success'
                  : 'border-border bg-hover text-muted'
              "
            >
              {{ agentPhaseLabel(agentUiPhase(agent)) }}
            </span>
          </div>

          <p v-if="agent.last_probe_error" class="text-xs text-danger truncate" :title="agent.last_probe_error">
            {{ agent.last_probe_error }}
          </p>

          <div class="flex flex-wrap gap-2">
            <button
              v-if="canOpenCloudAgent(agent)"
              type="button"
              class="h-8 px-3 rounded-lg bg-accent text-xs font-medium text-white hover:opacity-95 cursor-pointer disabled:opacity-50"
              :disabled="openingAgentId === agent.id"
              @click="onOpenAgent(agent)"
            >
              {{ openingAgentId === agent.id ? '打开中…' : '打开' }}
            </button>
            <template v-if="openWindowMap[agent.id]">
              <button
                type="button"
                class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover cursor-pointer"
                @click="onFocusAgent(agent.id)"
              >
                切换到云窗口
              </button>
              <button
                type="button"
                class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover cursor-pointer"
                @click="onCloseWindow(agent.id)"
              >
                关闭云窗口
              </button>
            </template>
            <button
              v-if="agentUiPhase(agent) === 'running'"
              type="button"
              class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover cursor-pointer"
              @click="startRenew(agent)"
            >
              续费
            </button>
            <button
              v-if="agent.instance_status !== 'released' && agent.instance_status !== 'releasing'"
              type="button"
              class="h-8 px-3 rounded-lg border border-danger/30 text-xs text-danger hover:bg-danger/10 cursor-pointer"
              @click="releaseAgent = agent"
            >
              释放
            </button>
          </div>
        </div>
      </div>
    </template>

    <!-- Purchase modal -->
    <div
      v-if="purchaseOpen"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 p-4"
      @click.self="purchaseOpen = false"
    >
      <div class="w-full max-w-md rounded-xl border border-border bg-[hsl(var(--card))] p-5 space-y-4 shadow-xl">
        <h4 class="text-sm font-semibold">购买云主机</h4>
        <label class="block text-xs text-muted">
          地域
          <select v-model="purchaseRegionId" class="mt-1 w-full h-9 rounded-lg border border-border bg-transparent px-2 text-sm" @change="loadPurchasePricing">
            <option v-for="r in regions" :key="r.id" :value="r.id">{{ r.name_zh }}</option>
          </select>
        </label>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="tab in ([
              ['hourly_trial', '试用'],
              ['hourly_spot', '标准'],
              ['monthly', '包月'],
              ['yearly', '包年']
            ] as const)"
            :key="tab[0]"
            type="button"
            class="h-7 px-3 rounded-full text-xs border cursor-pointer"
            :class="purchaseTab === tab[0] ? 'border-accent bg-accent/10 text-accent' : 'border-border'"
            @click="purchaseTab = tab[0]"
          >
            {{ tab[1] }}
          </button>
        </div>
        <label v-if="purchaseTab === 'hourly_trial' || purchaseTab === 'hourly_spot'" class="block text-xs text-muted">
          小时数
          <input v-model="purchaseHours" type="number" min="1" class="mt-1 w-full h-9 rounded-lg border border-border bg-transparent px-2 text-sm" />
        </label>
        <label v-else-if="purchaseTab === 'monthly'" class="block text-xs text-muted">
          月数
          <input v-model="purchaseMonthlyQty" type="number" min="1" class="mt-1 w-full h-9 rounded-lg border border-border bg-transparent px-2 text-sm" />
        </label>
        <label v-else class="block text-xs text-muted">
          年数
          <input v-model="purchaseYearlyQty" type="number" min="1" class="mt-1 w-full h-9 rounded-lg border border-border bg-transparent px-2 text-sm" />
        </label>
        <p v-if="purchasePreview" class="text-sm">
          应付 <span class="font-semibold text-accent">{{ purchasePreview.paid_yuan }}</span> 元
        </p>
        <div class="flex justify-end gap-2">
          <button type="button" class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover" @click="purchaseOpen = false">取消</button>
          <button type="button" class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover" @click="runPurchasePreview">试算</button>
          <button
            type="button"
            class="h-8 px-3 rounded-lg bg-accent text-xs font-medium text-white disabled:opacity-50"
            :disabled="purchaseSubmitting || !purchasePreview"
            @click="confirmPurchase"
          >
            {{ purchaseSubmitting ? '提交中…' : '确认购买' }}
          </button>
        </div>
      </div>
    </div>

    <!-- Renew modal -->
    <div
      v-if="renewAgent"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 p-4"
      @click.self="renewAgent = null"
    >
      <div class="w-full max-w-sm rounded-xl border border-border bg-[hsl(var(--card))] p-5 space-y-4">
        <h4 class="text-sm font-semibold">续费云主机</h4>
        <p class="text-xs text-muted">沿用当前套餐类型与时长参数</p>
        <p v-if="renewPreview" class="text-sm">应付 {{ renewPreview.paid_yuan }} 元</p>
        <div class="flex justify-end gap-2">
          <button type="button" class="h-8 px-3 rounded-lg border border-border text-xs" @click="renewAgent = null">取消</button>
          <button type="button" class="h-8 px-3 rounded-lg border border-border text-xs" @click="runRenewPreview">试算</button>
          <button type="button" class="h-8 px-3 rounded-lg bg-accent text-xs text-white disabled:opacity-50" :disabled="renewSubmitting" @click="confirmRenew">
            {{ renewSubmitting ? '提交中…' : '确认续费' }}
          </button>
        </div>
      </div>
    </div>

    <!-- Release confirm -->
    <div
      v-if="releaseAgent"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 p-4"
      @click.self="releaseAgent = null"
    >
      <div class="w-full max-w-sm rounded-xl border border-border bg-[hsl(var(--card))] p-5 space-y-4">
        <h4 class="text-sm font-semibold text-danger">释放云主机？</h4>
        <p class="text-xs text-muted">释放后实例将被销毁，数据无法恢复。</p>
        <div class="flex justify-end gap-2">
          <button type="button" class="h-8 px-3 rounded-lg border border-border text-xs" @click="releaseAgent = null">取消</button>
          <button type="button" class="h-8 px-3 rounded-lg bg-danger text-xs text-white disabled:opacity-50" :disabled="releaseSubmitting" @click="confirmRelease">
            {{ releaseSubmitting ? '释放中…' : '确认释放' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
