import { invoke } from '@tauri-apps/api/core'

export type PackageKind = 'hourly_trial' | 'hourly_spot' | 'monthly' | 'yearly'
export type AgentStatusFilter = 'running' | 'starting' | 'released' | 'all'

export interface PlatformMe {
  balance_yuan: string
  nickname?: string | null
  token_quota_exhausted?: boolean
}

export interface ShopRegion {
  id: string
  code: string
  name_zh: string
  sort_order: number
}

export interface MachineTierPublic {
  id: string
  label_zh: string
  description_zh?: string
  vcpu?: number | null
  memory_gb?: number | null
  disk_gb?: number | null
  price_multiplier: string
  is_default?: boolean
}

export interface ShopPricing {
  trial_hourly_rate_yuan: string
  hourly_rate_yuan: string
  monthly_yuan: string
  yearly_yuan: string
  trial_hourly_public_note?: string
  hourly_public_note?: string
  machine_tiers?: MachineTierPublic[]
}

export interface ShopPreviewResult {
  gross_yuan: string
  discount_yuan: string
  paid_yuan: string
  discount_percent_applied: number
  pricing: ShopPricing
}

export interface ServerRegionBrief {
  id: string
  code: string
  name_zh: string
}

export interface CloudAgent {
  id: string
  machine_tier_id?: string | null
  package_kind?: string | null
  hours_purchased?: number | null
  prepaid_months?: number | null
  prepaid_years?: number | null
  expires_at?: string | null
  instance_status: string
  app_status: string
  ecs_instance_id?: string | null
  public_ip?: string | null
  private_ip?: string | null
  last_probe_error?: string | null
  console_url?: string | null
  created_at: string
  server_region?: ServerRegionBrief | null
}

export interface AgentPhaseCounts {
  running: number
  starting: number
  releasing: number
  other: number
  released: number
}

export interface AgentListPage {
  items: CloudAgent[]
  total: number
  page: number
  page_size: number
  phase_counts: AgentPhaseCounts
}

export interface ShopPurchaseResult {
  agent: CloudAgent
  order_id: string
  balance_yuan: string
}

export interface ShopPreviewRequest {
  region_id: string
  package_kind: PackageKind
  hours?: number | null
  quantity?: number | null
  machine_tier_id?: string | null
}

export async function getCloudPlatformMe(): Promise<PlatformMe> {
  return await invoke<PlatformMe>('get_cloud_platform_me')
}

export async function listCloudShopRegions(): Promise<ShopRegion[]> {
  return await invoke<ShopRegion[]>('list_cloud_shop_regions')
}

export async function getCloudShopPricing(regionId: string): Promise<ShopPricing> {
  return await invoke<ShopPricing>('get_cloud_shop_pricing', { regionId })
}

export async function previewCloudShop(body: ShopPreviewRequest): Promise<ShopPreviewResult> {
  return await invoke<ShopPreviewResult>('preview_cloud_shop', { body })
}

export async function purchaseCloudAgent(body: ShopPreviewRequest): Promise<ShopPurchaseResult> {
  return await invoke<ShopPurchaseResult>('purchase_cloud_agent', { body })
}

export async function listCloudAgents(
  page: number,
  pageSize: number,
  status: AgentStatusFilter
): Promise<AgentListPage> {
  return await invoke<AgentListPage>('list_cloud_agents', { page, pageSize, status })
}

export async function renewCloudAgent(
  agentId: string,
  body: ShopPreviewRequest
): Promise<ShopPurchaseResult> {
  return await invoke<ShopPurchaseResult>('renew_cloud_agent', { agentId, body })
}

export async function releaseCloudAgent(agentId: string): Promise<CloudAgent> {
  return await invoke<CloudAgent>('release_cloud_agent', { agentId })
}

export async function openCloudAgent(agentId: string): Promise<void> {
  await invoke('open_cloud_agent', { agentId })
}

export async function focusCloudAgent(agentId: string): Promise<void> {
  await invoke('focus_cloud_agent', { agentId })
}

export async function closeCloudAgent(agentId: string): Promise<void> {
  await invoke('close_cloud_agent', { agentId })
}

export async function isCloudAgentWindowOpen(agentId: string): Promise<boolean> {
  return await invoke<boolean>('is_cloud_agent_window_open', { agentId })
}

export function agentUiPhase(agent: CloudAgent): string {
  if (agent.instance_status === 'released') return 'released'
  if (agent.instance_status === 'releasing') return 'releasing'
  if (agent.instance_status === 'instance_failed') return 'failed'
  if (
    agent.instance_status === 'instance_running' &&
    agent.app_status === 'app_ready'
  ) {
    return 'running'
  }
  if (
    agent.instance_status === 'provisioning' ||
    (agent.instance_status === 'instance_running' &&
      (agent.app_status === 'app_unknown' || agent.app_status === 'app_probing'))
  ) {
    return 'starting'
  }
  return 'other'
}

export function agentPhaseLabel(phase: string): string {
  switch (phase) {
    case 'running':
      return '运行中'
    case 'starting':
      return '启动中'
    case 'releasing':
      return '释放中'
    case 'released':
      return '已释放'
    case 'failed':
      return '启动失败'
    default:
      return '异常'
  }
}

export function canOpenCloudAgent(agent: CloudAgent): boolean {
  return (
    agent.app_status === 'app_ready' &&
    Boolean(agent.console_url?.trim()) &&
    agent.instance_status === 'instance_running'
  )
}

export function formatApiError(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e)
  if (msg.includes('token_quota_exhausted')) return 'Token 额度已用尽，请前往官网充值'
  if (msg.includes('insufficient_balance')) return '账户余额不足'
  if (msg.includes('platform_login_required') || msg.includes('请先登录')) {
    return '请先登录 Pointer 平台账户'
  }
  return msg
}
