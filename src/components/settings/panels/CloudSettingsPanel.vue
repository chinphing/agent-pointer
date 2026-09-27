<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Loader2, RefreshCw, X, Minus, Plus, Sparkles } from 'lucide-vue-next'
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
import { isBalanceExhaustedMessage, openPlatformBillingPage } from '../../../lib/platformUrls'

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
const purchasePreviewLoading = ref(false)
const purchaseSubmitting = ref(false)

let purchasePreviewTimer: ReturnType<typeof setTimeout> | null = null
let purchasePreviewSeq = 0

const renewAgent = ref<CloudAgent | null>(null)
const renewPreview = ref<ShopPreviewResult | null>(null)
const renewSubmitting = ref(false)

const releaseAgent = ref<CloudAgent | null>(null)
const releaseSubmitting = ref(false)

const { t } = useI18n()

const openingAgentId = ref<string | null>(null)

const loggedIn = computed(() => platformAuth.session.logged_in)

const purchasePackageOptions = computed(() => {
  const p = purchasePricing.value
  return [
    {
      kind: 'hourly_trial' as PackageKind,
      label: t('settings.cloud.planTrial'),
      price: p?.trial_hourly_rate_yuan,
      unit: t('settings.cloud.yuanPerHour'),
      note: p?.trial_hourly_public_note
    },
    {
      kind: 'hourly_spot' as PackageKind,
      label: t('settings.cloud.planHourly'),
      price: p?.hourly_rate_yuan,
      unit: t('settings.cloud.yuanPerHour'),
      note: p?.hourly_public_note
    },
    {
      kind: 'monthly' as PackageKind,
      label: t('settings.cloud.planMonthly'),
      price: p?.monthly_yuan,
      unit: t('settings.cloud.yuanPerMonth'),
      note: undefined
    },
    {
      kind: 'yearly' as PackageKind,
      label: t('settings.cloud.planYearly'),
      price: p?.yearly_yuan,
      unit: t('settings.cloud.yuanPerYear'),
      note: undefined
    }
  ]
})

const activePurchasePackageNote = computed(() => {
  const opt = purchasePackageOptions.value.find(o => o.kind === purchaseTab.value)
  return opt?.note?.trim() || ''
})

const purchaseDurationLabel = computed(() => {
  if (purchaseTab.value === 'hourly_trial' || purchaseTab.value === 'hourly_spot') return t('settings.cloud.durationHours')
  if (purchaseTab.value === 'monthly') return t('settings.cloud.durationMonths')
  return t('settings.cloud.durationYears')
})

const statusFilterOptions = computed(() => ([
  ['running', t('settings.cloud.status.running')],
  ['starting', t('settings.cloud.status.starting')],
  ['released', t('settings.cloud.status.released')],
  ['all', t('settings.cloud.status.all')]
] as const))


const purchaseDurationValue = computed({
  get() {
    if (purchaseTab.value === 'monthly') return purchaseMonthlyQty.value
    if (purchaseTab.value === 'yearly') return purchaseYearlyQty.value
    return purchaseHours.value
  },
  set(v: string) {
    if (purchaseTab.value === 'monthly') purchaseMonthlyQty.value = v
    else if (purchaseTab.value === 'yearly') purchaseYearlyQty.value = v
    else purchaseHours.value = v
  }
})

function adjustPurchaseDuration(delta: number) {
  const raw = parseInt(purchaseDurationValue.value, 10)
  const next = Number.isFinite(raw) ? raw + delta : 1
  purchaseDurationValue.value = String(Math.max(1, next))
}

function openPurchaseModal() {
  purchaseOpen.value = true
  purchasePreview.value = null
  void loadPurchasePricing()
  schedulePurchasePreview()
}

function closePurchaseModal() {
  purchaseOpen.value = false
  purchasePreview.value = null
  purchasePreviewLoading.value = false
  if (purchasePreviewTimer) {
    clearTimeout(purchasePreviewTimer)
    purchasePreviewTimer = null
  }
}

function schedulePurchasePreview() {
  if (!purchaseOpen.value) return
  if (purchasePreviewTimer) clearTimeout(purchasePreviewTimer)
  purchasePreviewTimer = setTimeout(() => {
    purchasePreviewTimer = null
    void runPurchasePreview()
  }, 280)
}

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
    schedulePurchasePreview()
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
    purchasePreview.value = null
    purchasePreviewLoading.value = false
    return
  }
  const seq = ++purchasePreviewSeq
  purchasePreviewLoading.value = true
  try {
    const result = await previewCloudShop(body)
    if (seq === purchasePreviewSeq) {
      purchasePreview.value = result
    }
  } catch (e) {
    if (seq === purchasePreviewSeq) {
      purchasePreview.value = null
      error.value = formatApiError(e)
    }
  } finally {
    if (seq === purchasePreviewSeq) {
      purchasePreviewLoading.value = false
    }
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
    closePurchaseModal()
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
  void runRenewPreview()
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

watch(
  [purchaseTab, purchaseHours, purchaseMonthlyQty, purchaseYearlyQty, purchaseTierId],
  () => {
    if (purchaseOpen.value) schedulePurchasePreview()
  }
)

watch(loggedIn, v => {
  if (v) void loadAll()
})

onMounted(() => {
  if (loggedIn.value) void loadAll()
})
</script>

<template>
  <div class="space-y-5">
    <div v-if="!loggedIn" class="rounded-xl border border-border panel p-5 space-y-3">
      <p class="text-sm text-muted">{{ t('settings.cloud.loginHint') }}</p>
      <button
        type="button"
        class="h-8 px-4 rounded-lg bg-accent text-sm font-medium text-accent-foreground hover:opacity-95 cursor-pointer"
        @click="loginPlatformAccount"
      >
        {{ t('settings.cloud.browserLogin') }}
      </button>
    </div>

    <template v-else>
      <div
        v-if="error"
        class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger"
        role="alert"
      >
        <div>{{ error }}</div>
        <button
          v-if="isBalanceExhaustedMessage(error)"
          type="button"
          class="mt-1.5 inline-flex rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-accent-foreground hover:opacity-90 cursor-pointer"
          @click="openPlatformBillingPage"
        >
          {{ t('settings.cloud.goRecharge') }}
        </button>
      </div>

      <div class="flex flex-wrap items-center justify-between gap-3">
        <div v-if="me" class="text-sm flex flex-wrap items-center gap-2">
          <span>
            <span class="text-muted">{{ t('settings.cloud.balanceLabel') }}</span>
            <span class="font-semibold tabular-nums text-accent">{{ me.balance_yuan }}</span>
            <span class="text-muted text-xs"> {{ t('settings.cloud.yuan') }}</span>
          </span>
          <button
            type="button"
            class="h-7 px-2 rounded-lg border border-border text-[11px] hover:bg-hover cursor-pointer"
            @click="openPlatformBillingPage"
          >
            {{ t('settings.cloud.recharge') }}
          </button>
        </div>
        <div class="flex flex-wrap gap-2">
          <button
            type="button"
            class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover cursor-pointer inline-flex items-center gap-1"
            :disabled="loading"
            @click="loadAll"
          >
            <RefreshCw class="w-3.5 h-3.5" />{{ t('common.refresh') }}
          </button>
          <button
            type="button"
            class="h-8 px-3 rounded-lg bg-accent text-xs font-medium text-accent-foreground hover:opacity-95 cursor-pointer inline-flex items-center gap-1.5"
            @click="openPurchaseModal"
          >
            <Sparkles class="w-3.5 h-3.5" />
            {{ t('settings.cloud.buyCloudHost') }}
          </button>
        </div>
      </div>

      <div class="flex flex-wrap gap-2">
        <button
          v-for="f in statusFilterOptions"
          :key="f[0]"
          type="button"
          class="h-7 px-3 rounded-full text-xs border cursor-pointer transition-colors"
          :class="
            statusFilter === f[0]
              ? 'border-border bg-hover text-foreground'
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
        {{ t('settings.cloud.noInstances') }}
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
                {{ agent.server_region?.name_zh || t('settings.cloud.cloudHost') }}
              </p>
              <p class="text-xs text-muted mt-0.5">
                {{ t('settings.cloud.expiresAt', { value: agent.expires_at ? new Date(agent.expires_at).toLocaleString() : '—' }) }}
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
              class="h-8 px-3 rounded-lg bg-accent text-xs font-medium text-accent-foreground hover:opacity-95 cursor-pointer disabled:opacity-50"
              :disabled="openingAgentId === agent.id"
              @click="onOpenAgent(agent)"
            >
              {{ openingAgentId === agent.id ? t('settings.cloud.opening') : t('settings.cloud.open') }}
            </button>
            <template v-if="openWindowMap[agent.id]">
              <button
                type="button"
                class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover cursor-pointer"
                @click="onFocusAgent(agent.id)"
              >
                {{ t('settings.cloud.switchToCloudWindow') }}
              </button>
              <button
                type="button"
                class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover cursor-pointer"
                @click="onCloseWindow(agent.id)"
              >
                {{ t('settings.cloud.closeCloudWindow') }}
              </button>
            </template>
            <button
              v-if="agentUiPhase(agent) === 'running'"
              type="button"
              class="h-8 px-3 rounded-lg border border-border text-xs hover:bg-hover cursor-pointer"
              @click="startRenew(agent)"
            >
              {{ t('settings.cloud.renew') }}
            </button>
            <button
              v-if="agent.instance_status !== 'released' && agent.instance_status !== 'releasing'"
              type="button"
              class="h-8 px-3 rounded-lg border border-danger/30 text-xs text-danger hover:bg-danger/10 cursor-pointer"
              @click="releaseAgent = agent"
            >
              {{ t('settings.cloud.release') }}
            </button>
          </div>
        </div>
      </div>
    </template>

    <!-- Purchase modal -->
    <div
      v-if="purchaseOpen"
      class="fixed inset-0 z-[60] flex items-end sm:items-center justify-center bg-foreground/32 backdrop-blur-[2px] p-3 sm:p-4"
      @click.self="closePurchaseModal"
    >
      <div
        class="w-full max-w-lg rounded-2xl border border-border bg-[hsl(var(--card))] shadow-2xl overflow-hidden"
        role="dialog"
        aria-modal="true"
        aria-labelledby="cloud-purchase-title"
      >
        <div class="flex items-start justify-between gap-3 px-5 pt-5 pb-4 border-b border-border/70">
          <div class="min-w-0">
            <h4 id="cloud-purchase-title" class="text-base font-semibold text-foreground">{{ t('settings.cloud.purchaseTitle') }}</h4>
            <p class="mt-1 text-xs text-muted leading-relaxed">{{ t('settings.cloud.purchaseHint') }}</p>
          </div>
          <button
            type="button"
            class="shrink-0 flex h-8 w-8 items-center justify-center rounded-lg border border-border text-muted hover:bg-hover hover:text-foreground transition-colors cursor-pointer"
            :aria-label="t('common.close')"
            @click="closePurchaseModal"
          >
            <X class="w-4 h-4" />
          </button>
        </div>

        <div class="px-5 py-4 space-y-5 max-h-[min(70vh,560px)] overflow-y-auto">
          <section class="space-y-2">
            <p class="text-xs font-medium text-muted">{{ t('settings.cloud.region') }}</p>
            <div
              class="flex gap-1 rounded-xl bg-hover/50 p-1"
              role="tablist"
              :aria-label="t('settings.cloud.region')"
            >
              <button
                v-for="r in regions"
                :key="r.id"
                type="button"
                role="tab"
                :aria-selected="purchaseRegionId === r.id"
                class="min-w-0 flex-1 rounded-lg px-3 py-2 text-center text-sm transition-all cursor-pointer"
                :class="
                  purchaseRegionId === r.id
                    ? 'bg-[hsl(var(--card-elevated))] font-medium text-foreground shadow-sm ring-1 ring-border/80'
                    : 'text-muted hover:text-foreground hover:bg-hover/80'
                "
                @click="purchaseRegionId = r.id"
              >
                {{ r.name_zh }}
              </button>
            </div>
          </section>

          <section class="space-y-2">
            <p class="text-xs font-medium text-muted">{{ t('settings.cloud.planType') }}</p>
            <div class="grid grid-cols-2 gap-2">
              <button
                v-for="opt in purchasePackageOptions"
                :key="opt.kind"
                type="button"
                class="rounded-xl border p-3 text-left transition-all cursor-pointer"
                :class="
                  purchaseTab === opt.kind
                    ? 'border-accent/45 bg-accent/8 ring-1 ring-accent/25'
                    : 'border-border bg-[hsl(var(--card-elevated))] hover:border-accent/25 hover:bg-hover/40'
                "
                @click="purchaseTab = opt.kind"
              >
                <span class="block text-sm font-medium text-foreground">{{ opt.label }}</span>
                <span v-if="opt.price" class="mt-1 block text-xs tabular-nums text-accent">
                  {{ opt.price }}
                  <span class="text-muted font-normal">{{ opt.unit }}</span>
                </span>
                <span v-else class="mt-1 block text-xs text-muted">{{ t('settings.cloud.loadingUnitPrice') }}</span>
              </button>
            </div>
            <p
              v-if="activePurchasePackageNote"
              class="rounded-lg border border-border/70 bg-hover/30 px-3 py-2 text-[11px] leading-relaxed text-muted"
            >
              {{ activePurchasePackageNote }}
            </p>
          </section>

          <section class="space-y-2">
            <p class="text-xs font-medium text-muted">{{ purchaseDurationLabel }}</p>
            <div class="flex items-center gap-2">
              <button
                type="button"
                class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border text-muted hover:bg-hover cursor-pointer disabled:opacity-40"
                :aria-label="t('settings.cloud.decrease')"
                @click="adjustPurchaseDuration(-1)"
              >
                <Minus class="w-4 h-4" />
              </button>
              <input
                v-model="purchaseDurationValue"
                type="number"
                min="1"
                class="h-9 w-full rounded-lg border border-border bg-[hsl(var(--card-elevated))] px-3 text-center text-sm tabular-nums text-foreground outline-none focus:border-accent/50 focus:ring-2 focus:ring-accent/15"
              />
              <button
                type="button"
                class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border text-muted hover:bg-hover cursor-pointer"
                :aria-label="t('settings.cloud.increase')"
                @click="adjustPurchaseDuration(1)"
              >
                <Plus class="w-4 h-4" />
              </button>
            </div>
          </section>

          <section
            class="rounded-xl border px-4 py-3 transition-colors"
            :class="
              purchasePreview
                ? 'border-accent/30 bg-accent/8'
                : 'border-dashed border-border bg-hover/20'
            "
          >
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="text-xs text-muted">{{ t('settings.cloud.amountDue') }}</p>
                <p
                  v-if="purchasePreviewLoading"
                  class="mt-1 text-sm text-muted inline-flex items-center gap-1.5"
                >
                  <Loader2 class="w-3.5 h-3.5 animate-spin" />
                  {{ t('settings.cloud.calculating') }}
                </p>
                <p v-else-if="purchasePreview" class="mt-0.5 text-2xl font-semibold tabular-nums text-accent">
                  {{ purchasePreview.paid_yuan }}
                  <span class="text-sm font-normal text-muted">{{ t('settings.cloud.yuan') }}</span>
                </p>
                <p v-else class="mt-1 text-sm text-muted">{{ t('settings.cloud.enterDurationHint') }}</p>
              </div>
              <div v-if="me && purchasePreview" class="text-right text-[11px] text-muted leading-relaxed">
                <p>{{ t('settings.cloud.currentBalance', { balance: me.balance_yuan }) }}</p>
              </div>
            </div>
            <p
              v-if="purchasePreview && purchasePreview.discount_percent_applied > 0"
              class="mt-2 text-[11px] text-muted"
            >
              {{ t('settings.cloud.discountApplied', { amount: purchasePreview.discount_yuan, percent: purchasePreview.discount_percent_applied }) }}
            </p>
          </section>
        </div>

        <div class="flex flex-wrap items-center justify-end gap-2 px-5 py-4 border-t border-border/70 bg-[hsl(var(--card-elevated))]">
          <button
            type="button"
            class="h-9 px-4 rounded-lg border border-border text-xs text-muted hover:bg-hover cursor-pointer"
            @click="closePurchaseModal"
          >
            {{ t('common.cancel') }}
          </button>
          <button
            type="button"
            class="h-9 px-4 rounded-lg bg-accent text-xs font-medium text-accent-foreground hover:opacity-95 disabled:opacity-50 cursor-pointer inline-flex items-center gap-1.5"
            :disabled="purchaseSubmitting || purchasePreviewLoading || !purchasePreview"
            @click="confirmPurchase"
          >
            <Loader2 v-if="purchaseSubmitting" class="w-3.5 h-3.5 animate-spin" />
            {{ purchaseSubmitting ? t('settings.cloud.submitting') : t('settings.cloud.confirmPurchase') }}
          </button>
        </div>
      </div>
    </div>

    <!-- Renew modal -->
    <div
      v-if="renewAgent"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-foreground/32 p-4"
      @click.self="renewAgent = null"
    >
      <div class="w-full max-w-sm rounded-xl border border-border bg-[hsl(var(--card))] p-5 space-y-4">
        <h4 class="text-sm font-semibold">{{ t('settings.cloud.renewTitle') }}</h4>
        <p class="text-xs text-muted">{{ t('settings.cloud.renewHint') }}</p>
        <p v-if="renewPreview" class="text-sm">{{ t('settings.cloud.renewAmountDue', { amount: renewPreview.paid_yuan }) }}</p>
        <div class="flex justify-end gap-2">
          <button type="button" class="h-8 px-3 rounded-lg border border-border text-xs" @click="renewAgent = null">{{ t('common.cancel') }}</button>
          <button
            type="button"
            class="h-8 px-3 rounded-lg bg-accent text-xs text-accent-foreground disabled:opacity-50"
            :disabled="renewSubmitting || !renewPreview"
            @click="confirmRenew"
          >
            {{ renewSubmitting ? t('settings.cloud.submitting') : t('settings.cloud.confirmRenew') }}
          </button>
        </div>
      </div>
    </div>

    <!-- Release confirm -->
    <div
      v-if="releaseAgent"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-foreground/32 p-4"
      @click.self="releaseAgent = null"
    >
      <div class="w-full max-w-sm rounded-xl border border-border bg-[hsl(var(--card))] p-5 space-y-4">
        <h4 class="text-sm font-semibold text-danger">{{ t('settings.cloud.releaseTitle') }}</h4>
        <p class="text-xs text-muted">{{ t('settings.cloud.releaseHint') }}</p>
        <div class="flex justify-end gap-2">
          <button type="button" class="h-8 px-3 rounded-lg border border-border text-xs" @click="releaseAgent = null">{{ t('common.cancel') }}</button>
          <button type="button" class="h-8 px-3 rounded-lg bg-danger text-xs text-white disabled:opacity-50" :disabled="releaseSubmitting" @click="confirmRelease">
            {{ releaseSubmitting ? t('settings.cloud.releasing') : t('settings.cloud.confirmRelease') }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
