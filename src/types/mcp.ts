/**
 * 全局（非插件）MCP 管理 API 类型（P2b）。
 * 对应 Rust `pointer_core::chat_service::app_state::GlobalMcpView`。
 */

/** 单个 MCP 工具简要信息（注册后可见；对应 Rust McpToolBrief）。 */
export interface McpToolBrief {
  name: string
  description: string
}

export interface GlobalMcpServerView {
  name: string
  command: string
  transport: string
  /** healthy | crashed | degraded | stopped */
  status: string
  restartCount: number
  lastError?: string | null
  args: string[]
  env: Record<string, string>
  url?: string | null
  headers?: Record<string, string> | null
  /** 该服务当前已注册的工具（未连接/注册失败时为空数组） */
  tools: McpToolBrief[]
}

export interface GlobalMcpView {
  servers: GlobalMcpServerView[]
  enabled: boolean
}

/** 界面可编辑的 MCP server 声明（对应 Rust McpServerDecl）。 */
export interface McpServerDecl {
  name: string
  /** stdio（本机命令） | http（远程服务） */
  transport: string
  /** stdio 传输：启动命令 */
  command: string
  args: string[]
  env: Record<string, string>
  /** http 传输：远程服务地址 */
  url?: string | null
  /** http 传输：附加请求头 */
  headers?: Record<string, string> | null
}
