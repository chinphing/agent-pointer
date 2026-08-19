/**
 * Pointer 插件（P1）API 类型。
 * 对应 Rust `pointer_core::plugins` 的序列化视图。
 */

export type PluginStatus =
  | 'discovered'
  | 'rejected'
  | 'enabled'
  | 'disabled'
  | 'needs_reauth'

export interface PluginView {
  pluginId: string
  name: string
  version: string
  description: string
  isUserLevel: boolean
  status: PluginStatus
  statusReason?: string | null
  isAuthorized: boolean
  isEnabled: boolean
  /** 能力单元清单（skills/agents/rules/hooks/mcp_servers/tools(n)）。 */
  capabilities: string[]
}

export interface ImportReport {
  pluginId: string
  pluginName: string
  targetDir: string
  converted: string[]
  skipped: string[]
  unmapped: string[]
}

export type PluginKind = 'pointer' | 'codex' | 'claude'

export interface DiscoveredPlugin {
  kind: PluginKind
  root: string
  manifest: string
  name: string
  version: string
  description: string
}

export interface ExternalPluginSource {
  id: string
  label: string
  path: string
  pluginName: string
  pluginVersion: string
  description: string
}

export interface ExternalPluginsProbeResult {
  sources: ExternalPluginSource[]
  total: number
}
