import { invoke } from '@tauri-apps/api/core'

/**
 * 控制面的 `/auth/me` 视图。云主机商城已下线，这里只剩账户余额/额度信息，
 * 供余额条（`usePlatformBalance` → `BalanceChip` / `AccountMenu`）使用。
 */
export interface PlatformMe {
  balance_yuan: string
  nickname?: string | null
  token_quota_exhausted?: boolean
}

export async function getCloudPlatformMe(): Promise<PlatformMe> {
  return await invoke<PlatformMe>('get_cloud_platform_me')
}
