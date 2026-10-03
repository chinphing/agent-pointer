import type { AgentTrace, ChatMessage, ToolCall } from '../types/chat'
import { toolCallBaseName } from './messageTooling'
import { isScopedSubMessage } from './subAgentMessages'
import { subTracesForMessage } from './subAgentSession'
import { buildSubAgentTraceTree } from './subAgentTraceTree'

/**
 * Turn-scoped spawn ownership tree (P0 of「归属是数据关系」).
 *
 * Why this exists: nesting **is** a data relation — a `run_subagent` row owns a child
 * trace — but the renderer used to resolve it **per message**: a trace and its host row
 * had to sit on the same `ChatMessage`, otherwise the frame fell into a flat "orphan"
 * bucket appended after the body. Background spawns (`run_subagent` + `job.await`) and
 * the collapsed projection both break that co-location, so several agents' frames ended
 * up stacked under whatever row happened to be drawn last.
 *
 * Invariants this selector establishes (views may consume them, never re-derive):
 *
 * - **I1 — ownership is data**: `AgentTrace.parentToolCallId` resolves to a host row over
 *   the **whole turn**, wherever that row is carried (lead message / trailing tool group /
 *   the issuer's scoped row). Only `run_subagent` rows are hosts; a `job` row never is.
 * - **I2 — collapse is a projection**: a hidden surface never drops a node. `planSurfaces`
 *   lifts the frame to the nearest drawn container and reports `detached` when the chain
 *   reaches no drawn surface.
 * - **I3 — nothing is silently lost**: traces whose host row is absent from the turn are
 *   reported as `unbound` (legacy / degraded data) instead of disappearing.
 *
 * Pure: no store, no Vue, no side effects — unit-testable and memo-friendly per turn.
 */

/** Only this tool issues children. */
export const SPAWN_TOOL_BASE_NAME = 'run_subagent'

export type SpawnRowRef = {
  rowId: string
  row: ToolCall
  /** Message whose `toolCalls` carry the row (lead message, or the issuer's scoped row). */
  messageId: string
}

export type SpawnNode = {
  /** `row:<rowId>` when the host row resolved, `trace:<traceId>` otherwise. */
  key: string
  rowId: string
  row: ToolCall | null
  rowMessageId: string
  trace: AgentTrace | null
  /** Nested spawns issued **by** this child (their host rows live on its scoped rows). */
  children: SpawnNode[]
  /** Host row absent from the turn — legacy / degraded data, must stay visible. */
  unbound: boolean
}

export interface SurfacePlanInput {
  /** Every surface the view draws: row ids **and** container keys (collapsed group / block). */
  drawn: ReadonlySet<string> | readonly string[]
  /**
   * Surface a key belongs to when the key itself is not drawn — e.g. a row inside a
   * collapsed tool group maps to the group's key. Defaults to identity (own surface).
   */
  containerOf?: (surfaceKey: string) => string
}

export interface SurfacePlan {
  /** surface key → frames to draw there, in turn order. */
  framesBySurface: Map<string, SpawnNode[]>
  /** Node keys whose own row is not drawn (frame lifted to a container surface). */
  lifted: Set<string>
  /** Row hidden and no drawn container in the chain — the view must surface these explicitly. */
  detached: SpawnNode[]
  /** Traces with no host row anywhere in the turn. */
  unbound: SpawnNode[]
}

export interface TurnSpawnTree {
  /** Every node, lead layer first (same order as `buildSubAgentTraceTree`). */
  nodes: SpawnNode[]
  /** Nodes whose parent node is not part of the turn — the ones a view places on a surface. */
  roots: SpawnNode[]
  nodeForRow(rowId: string): SpawnNode | null
  nodeForTrace(traceId: string): SpawnNode | null
  childrenOfRow(rowId: string): SpawnNode[]
  /** Spawn rows with no trace yet (queued / just started). */
  pendingRows: SpawnRowRef[]
  /** Host rows claimed by more than one trace (anomaly; first claim wins, order-stable). */
  duplicatedHostRowIds: string[]
  unbound: SpawnNode[]
  planSurfaces(input: SurfacePlanInput): SurfacePlan
}

function trimId(value: string | undefined | null): string {
  return (value ?? '').trim()
}

function isSpawnRow(tc: ToolCall): boolean {
  return toolCallBaseName(tc.name) === SPAWN_TOOL_BASE_NAME
}

/**
 * Build the ownership tree for **one turn**.
 *
 * Pass the turn's full, unprojected data: `messages` (lead rounds), `scopedRows` (child
 * rows of the same turn — they carry nested spawn rows and nested traces) and
 * `extraToolCallsByMessageId` for rows the collapse projection moved onto another
 * message. Projecting first (hiding rows) must never change the tree — that is I2.
 */
export function buildTurnSpawnTree(input: {
  messages?: readonly ChatMessage[] | null
  scopedRows?: readonly ChatMessage[] | null
  extraToolCallsByMessageId?: ReadonlyMap<string, readonly ToolCall[]> | null
}): TurnSpawnTree {
  const rows = new Map<string, SpawnRowRef>()
  const rowOrder: string[] = []

  function addRow(tc: ToolCall, messageId: string): void {
    if (!isSpawnRow(tc)) return
    const rowId = trimId(tc.id)
    if (!rowId || rows.has(rowId)) return
    rows.set(rowId, { rowId, row: tc, messageId })
    rowOrder.push(rowId)
  }

  function addRowsFrom(message: ChatMessage): void {
    for (const tc of message.toolCalls ?? []) addRow(tc, message.id)
  }

  for (const message of input.messages ?? []) addRowsFrom(message)
  for (const row of input.scopedRows ?? []) addRowsFrom(row)
  for (const [messageId, calls] of input.extraToolCallsByMessageId ?? []) {
    for (const tc of calls) addRow(tc, messageId)
  }

  // Traces live on their own layer's row: depth 1 on a lead message, deeper on the
  // issuing worker's scoped row. Scoped rows are passed to the tree so it can also
  // rebuild the parent↔child axis (legacy rows fall back to the owner instance).
  const leadTraces: AgentTrace[] = []
  for (const message of input.messages ?? []) {
    if (isScopedSubMessage(message)) continue
    for (const trace of message.agentTrace ?? []) leadTraces.push(trace)
  }
  const traceTree = buildSubAgentTraceTree({
    leadTraces,
    scopedRows: (input.scopedRows ?? []).filter(isScopedSubMessage)
  })

  const nodes: SpawnNode[] = []
  const nodeByRow = new Map<string, SpawnNode>()
  const nodeByTrace = new Map<string, SpawnNode>()
  const claimedRowIds = new Set<string>()
  const duplicatedHostRowIds: string[] = []

  for (const trace of traceTree.all) {
    const traceId = trimId(trace.id)
    if (!traceId) continue
    const hostRowId = trimId(trace.parentToolCallId)
    const host = hostRowId ? rows.get(hostRowId) : undefined
    const claimed = !!host && claimedRowIds.has(hostRowId)
    if (claimed) duplicatedHostRowIds.push(hostRowId)
    const resolved = host && !claimed ? host : undefined
    const node: SpawnNode = resolved
      ? {
          key: `row:${resolved.rowId}`,
          rowId: resolved.rowId,
          row: resolved.row,
          rowMessageId: resolved.messageId,
          trace,
          children: [],
          unbound: false
        }
      : {
          key: `trace:${traceId}`,
          rowId: '',
          row: null,
          rowMessageId: '',
          trace,
          children: [],
          unbound: true
        }
    if (resolved) {
      claimedRowIds.add(resolved.rowId)
      nodeByRow.set(resolved.rowId, node)
    }
    nodeByTrace.set(traceId, node)
    nodes.push(node)
  }

  const childKeys = new Set<string>()
  for (const node of nodes) {
    const parentId = trimId(node.trace?.parentTraceId)
    const parent = parentId ? nodeByTrace.get(parentId) : undefined
    if (!parent || parent === node) continue
    parent.children.push(node)
    childKeys.add(node.key)
  }

  const roots = nodes.filter(node => !childKeys.has(node.key))
  const unbound = nodes.filter(node => node.unbound)
  const pendingRows = rowOrder
    .map(rowId => rows.get(rowId)!)
    .filter(ref => !claimedRowIds.has(ref.rowId))

  function planSurfaces(input: SurfacePlanInput): SurfacePlan {
    const drawn = new Set<string>(
      Array.isArray(input.drawn)
        ? (input.drawn as readonly string[])
        : [...(input.drawn as ReadonlySet<string>)]
    )
    const containerOf = input.containerOf ?? ((surfaceKey: string) => surfaceKey)
    const framesBySurface = new Map<string, SpawnNode[]>()
    const lifted = new Set<string>()
    const detached: SpawnNode[] = []

    for (const node of roots) {
      if (node.unbound) continue
      let surface = node.rowId
      const walked = new Set<string>()
      while (!drawn.has(surface) && !walked.has(surface)) {
        walked.add(surface)
        const next = containerOf(surface)
        if (!next || next === surface) break
        surface = next
      }
      if (!drawn.has(surface)) {
        detached.push(node)
        continue
      }
      if (surface !== node.rowId) lifted.add(node.key)
      const list = framesBySurface.get(surface)
      if (list) list.push(node)
      else framesBySurface.set(surface, [node])
    }

    return { framesBySurface, lifted, detached, unbound }
  }

  return {
    nodes,
    roots,
    nodeForRow: (rowId: string) => nodeByRow.get(trimId(rowId)) ?? null,
    nodeForTrace: (traceId: string) => nodeByTrace.get(trimId(traceId)) ?? null,
    childrenOfRow: (rowId: string) => nodeByRow.get(trimId(rowId))?.children ?? [],
    pendingRows,
    duplicatedHostRowIds,
    unbound,
    planSurfaces
  }
}

// ---------------------------------------------------------------------------
// 回合渲染计划（P1）：一次算清「哪张行画在哪条消息上」与「谁的档案没地方挂」
// ---------------------------------------------------------------------------

export type TurnSpawnPlan = {
  tree: TurnSpawnTree
  /** 本回合画出来的所有行 id（折叠感知）。 */
  drawnRowIds: ReadonlySet<string>
  /** 每条消息各自画出来的行 id（折叠感知）。 */
  drawnRowIdsByMessage: ReadonlyMap<string, ReadonlySet<string>>
}

export type MessageSpawnFrames = {
  /** rowId → 挂在这张行下面的子帧（只含本消息画出来的行）。 */
  byRow: Map<string, AgentTrace[]>
  /** 宿主行存在，但本回合里没有画出来（被折叠隐藏）。 */
  unplaced: AgentTrace[]
  /** 宿主行在本回合里根本不存在（历史脏数据 / 缺行）。 */
  unbound: AgentTrace[]
}

const EMPTY_ROW_IDS: ReadonlySet<string> = new Set<string>()

/**
 * 组装一个回合的渲染计划。`drawnRowIdsByMessage` 由视图给出 —— 必须是**折叠投影之后**真正会画的行，
 * 这样「行被藏起来」才不会被误判成「行还在」。
 */
export function buildTurnSpawnPlan(input: {
  messages?: readonly ChatMessage[] | null
  scopedRows?: readonly ChatMessage[] | null
  extraToolCallsByMessageId?: ReadonlyMap<string, readonly ToolCall[]> | null
  drawnRowIdsByMessage?: ReadonlyMap<string, readonly string[]> | null
}): TurnSpawnPlan {
  const drawnRowIdsByMessage = new Map<string, ReadonlySet<string>>()
  const drawnRowIds = new Set<string>()
  for (const [messageId, rowIds] of input.drawnRowIdsByMessage ?? []) {
    const set = new Set<string>()
    for (const rowId of rowIds) {
      const id = trimId(rowId)
      if (!id) continue
      set.add(id)
      drawnRowIds.add(id)
    }
    drawnRowIdsByMessage.set(trimId(messageId), set)
  }
  return {
    tree: buildTurnSpawnTree({
      messages: input.messages,
      scopedRows: input.scopedRows,
      extraToolCallsByMessageId: input.extraToolCallsByMessageId
    }),
    drawnRowIds,
    drawnRowIdsByMessage
  }
}

/**
 * 这条消息要自己画的子帧。
 *
 * - `byRow`：**向树要**（`childrenOfRow`），而不是只看本消息的 `agentTrace` —— 这是 P1 的核心：
 *   谁把宿主行画出来，谁就把子帧画在它下面，与「档案被投递到哪条消息」无关。
 * - `unplaced` / `unbound`：本消息携带、但**本回合任何地方都没画出来**的档案 → 进显式未关联组。
 *   宿主行已在别处画出来的档案不会进这里（避免重复渲染）。
 */
export function framesForMessage(plan: TurnSpawnPlan, message: ChatMessage): MessageSpawnFrames {
  const drawnHere = plan.drawnRowIdsByMessage.get(message.id) ?? EMPTY_ROW_IDS
  const byRow = new Map<string, AgentTrace[]>()
  for (const rowId of drawnHere) {
    // 一张宿主行至多一个子帧；重复认领的那份会进 unbound（见 buildTurnSpawnTree）。
    const trace = plan.tree.nodeForRow(rowId)?.trace
    if (trace) byRow.set(rowId, [trace])
  }
  const unplaced: AgentTrace[] = []
  const unbound: AgentTrace[] = []
  for (const trace of subTracesForMessage(message)) {
    const rowId = trimId(trace.parentToolCallId)
    const node = rowId ? plan.tree.nodeForRow(rowId) : null
    if (!node) {
      // 没有宿主行：job 行 / 行不存在 / 重复认领 → 显式未关联组
      unbound.push(trace)
      continue
    }
    if (plan.drawnRowIds.has(rowId)) continue
    unplaced.push(trace)
  }
  return { byRow, unplaced, unbound }
}

export type MissingHostFetch = {
  traceId: string
  /** 取数锚点：该层 scoped 行所在的消息（嵌套层 = 父 worker 的 scoped 行 id）。 */
  anchorMessageId: string
  agentInstanceId?: string
}

/**
 * 「宿主行还没加载」的补拉清单（P0.1）。
 *
 * 回合索引只覆盖**内存里已加载**的 scoped 行，所以嵌套层（第 2 层及更深 —— 派发行写在父 worker
 * 的 scoped 行上）在未加载 / 被驱逐时解析不到宿主行，会被误判成「未关联」。这里给出需要补拉的
 * 档案，调用方用 `ensureScopedMessagesForTrace(anchorMessageId, traceId, agentInstanceId)`
 * **按锚精确拉一次**（不是全表扫）。
 *
 * 纯函数：不做副作用、不缓存；`requested` 由调用方维护，用来保证每个 trace 只拉一次。
 * 没有 `parentToolCallId` 的历史档案不补拉 —— 补了也解析不出宿主行。
 */
export function missingHostFetches(
  plan: TurnSpawnPlan,
  message: ChatMessage,
  requested?: ReadonlySet<string> | null
): MissingHostFetch[] {
  const out: MissingHostFetch[] = []
  for (const trace of subTracesForMessage(message)) {
    const traceId = trimId(trace.id)
    if (!traceId || requested?.has(traceId)) continue
    const rowId = trimId(trace.parentToolCallId)
    if (!rowId) continue
    if (plan.tree.nodeForRow(rowId)) continue
    const anchorMessageId = trimId(trace.anchorMessageId) || trimId(message.id)
    if (!anchorMessageId) continue
    out.push({ traceId, anchorMessageId, agentInstanceId: trace.agentInstanceId })
  }
  return out
}
