/**
 * 全局（非插件）MCP 管理 API 类型（P2b）。
 * 对应 Rust `pointer_core::chat_service::app_state::GlobalMcpView`。
 */

export interface GlobalMcpServerView {
  name: string
  command: string
  transport: string
  /** healthy | crashed | degraded | stopped */
  status: string
  restartCount: number
  lastError?: string | null
}

export interface GlobalMcpView {
  servers: GlobalMcpServerView[]
  enabled: boolean
}
