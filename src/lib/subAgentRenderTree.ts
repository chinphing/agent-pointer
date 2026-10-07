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
  /**
   * 本层的 head（例如「帧自己的 trace」）。它的宿主行在**上一层**，不在本层的行里；
   * 不 seed 的话它的子档案会因为「父找不到」整棵掉出树 —— 帧内挂载就会全部落进兜底。
   */
  seedTraces?: readonly AgentTrace[] | null
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
  for (const trace of input.seedTraces ?? []) leadTraces.push(trace)
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
  /** 本层的 head trace（见 `buildTurnSpawnTree` 的同名字段）。 */
  seedTraces?: readonly AgentTrace[] | null
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
      extraToolCallsByMessageId: input.extraToolCallsByMessageId,
      seedTraces: input.seedTraces
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
  const ownRows = isScopedSubMessage(message) ? ownToolRowIds(message) : EMPTY_ROW_IDS
  const byRow = new Map<string, AgentTrace[]>()
  for (const rowId of drawnHere) {
    // 一张宿主行至多一个子帧；重复认领的那份会进 unbound（见 buildTurnSpawnTree）。
    const trace = plan.tree.nodeForRow(rowId)?.trace
    if (trace) byRow.set(rowId, [trace])
  }
  // 帧内行：本消息**自己画**的工具行也要认领（见 `ownToolRowIds`）。回合级的
  // `drawnRowIdsByMessage` 只覆盖回合消息画出来的行，而 scoped 行由帧内的
  // `AssistantModelMessage` 自己画 —— 父 worker 的 `run_subagent` 行正是这一类。
  for (const rowId of ownRows) {
    if (byRow.has(rowId)) continue
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
    if (plan.drawnRowIds.has(rowId) || ownRows.has(rowId)) continue
    unplaced.push(trace)
  }
  return { byRow, unplaced, unbound }
}

/**
 * 本消息**自己会画**的工具行 id。
 *
 * 为什么需要它：`TurnSpawnPlan.drawnRowIdsByMessage` 由视图从 `displayedTurnEntries`
 * 建出来，只覆盖**回合级消息**画的行。子 agent 的行（scoped 行）画在帧里面，由帧内的
 * `AssistantModelMessage` 自己渲染，**不在**那份映射里。
 *
 * 缺了它，depth-2 的档案会同时错两次（真实 case：会话 `c7b8306b-3026-41d7-889d-c434dd74602f`
 * 的 3 个 explore 子帧）：
 * - `byRow` 空 → 子帧挂不到父 worker 的 `run_subagent` 行上（帧下无子行）；
 * - 又被判成「本回合任何地方都没画出来」→ 掉进孤儿组（帧头平铺 + 底部孤儿块）。
 *
 * 判据与 P1 一致：**谁画宿主行，谁画子帧**。只对 **scoped 行**成立（调用点用
 * `isScopedSubMessage` 收口）：帧内的消息必然画自己的工具行，而**回合消息**的工具行可能被
 * 折叠隐藏 —— 那时宿主行没画出来，档案必须留在未关联组里（`subAgentRenderTree.test.ts`
 * 的「宿主行被折叠隐藏时进未关联组」守着这条）。
 */
function ownToolRowIds(message: ChatMessage): ReadonlySet<string> {
  const ids = new Set<string>()
  for (const tc of message.toolCalls ?? []) {
    const id = trimId(tc.id)
    if (id) ids.add(id)
  }
  return ids
}

export type MissingHostFetch = {
  /** 去重键：同一个父实例只拉一次。 */
  key: string
  /** 要拉的是**父实例**的行 —— 缺的宿主行（那次 `run_subagent` 调用）写在父的行里。 */
  traceId: string
  anchorMessageId: string
  agentInstanceId?: string
  /** 触发这次补拉的档案（诊断用）。 */
  traceIds: string[]
}

/**
 * 「宿主行还没加载」的补拉清单（P0.1）。
 *
 * 关键：缺的宿主行是**父级**发出的那次 `run_subagent` 调用 —— 按定义永远不在档案自己的行里。
 * 所以补拉的对象必须是**父 trace 对应的实例**
 * （`ensureScopedMessagesForTrace(父锚, 父 traceId, 父 instanceId)`），而不是这条档案自己。
 *
 * 父 trace 从本回合的树里找（第 1 层父就是 lead 消息上的档案）。找不到父 trace 时不请求 ——
 * 拉自己那一份对解析宿主行没有帮助。
 *
 * 纯函数；`requested`（键集合）由调用方维护，保证每个父实例只拉一次。
 */
export function missingHostFetches(
  plan: TurnSpawnPlan,
  message: ChatMessage,
  requested?: ReadonlySet<string> | null
): MissingHostFetch[] {
  const byKey = new Map<string, MissingHostFetch>()
  for (const trace of subTracesForMessage(message)) {
    const traceId = trimId(trace.id)
    if (!traceId) continue
    const rowId = trimId(trace.parentToolCallId)
    if (!rowId) continue
    if (plan.tree.nodeForRow(rowId)) continue
    const parentId = trimId(trace.parentTraceId)
    if (!parentId) continue
    const parent = plan.tree.nodeForTrace(parentId)?.trace
    if (!parent) continue
    const anchorMessageId = trimId(parent.anchorMessageId) || trimId(message.id)
    if (!anchorMessageId) continue
    const instanceId = trimId(parent.agentInstanceId) || trimId(parent.id)
    const key = `${anchorMessageId}\u0000${instanceId}`
    if (requested?.has(key)) continue
    const existing = byKey.get(key)
    if (existing) {
      existing.traceIds.push(traceId)
      continue
    }
    byKey.set(key, {
      key,
      traceId: trimId(parent.id) || parentId,
      anchorMessageId,
      // 与 SubAgentFrameHost 一致：没有 agentInstanceId 时用 trace id 兜底
      agentInstanceId: instanceId,
      traceIds: [traceId]
    })
  }
  return [...byKey.values()]
}

/**
 * 把「锚在给定消息上」的 scoped 行做**传递闭包**。
 *
 * 嵌套层的行锚在**父级的行 id** 上（父 worker 的 scoped 行），只取一层会漏掉第 2 层及更深 ——
 * 于是宿主行找不到、帧被误判成「未关联」。
 */
export function scopedRowClosure(
  seedIds: readonly string[],
  collectRowsForAnchors: (anchorIds: readonly string[]) => readonly ChatMessage[],
  maxDepth = 4
): ChatMessage[] {
  const out: ChatMessage[] = []
  const seen = new Set<string>()
  let frontier: string[] = []
  for (const raw of seedIds) {
    const id = trimId(raw)
    if (!id || seen.has(id)) continue
    seen.add(id)
    frontier.push(id)
  }
  for (let depth = 0; depth < maxDepth && frontier.length > 0; depth++) {
    const next: string[] = []
    for (const row of collectRowsForAnchors(frontier)) {
      const id = trimId(row.id)
      if (!id || seen.has(id)) continue
      seen.add(id)
      out.push(row)
      next.push(id)
    }
    frontier = next
  }
  return out
}

export type LayerSpawnFrames = {
  /** rowId → 挂在这张行下面的子帧（只含本层画出来的行；一行至多一个）。 */
  byRow: Map<string, AgentTrace[]>
  /** 本层任何行都没画出来的子帧（宿主行缺失 / 没画）→ 兜底块，不丢。 */
  detached: AgentTrace[]
}

/**
 * 一层（帧内）的子帧分组（P3）。
 *
 * 与 `framesForMessage` 同一套语义：谁画出行，谁就把子帧画在它下面；宿主行没画出来的进
 * `detached`。区别在于档案来源 —— 这里来自该层的 trace 树（`childTraces`），不是某条消息的
 * `agentTrace`。
 */
export function framesForLayer(
  plan: TurnSpawnPlan,
  traces: readonly AgentTrace[]
): LayerSpawnFrames {
  const allowed = new Set<string>()
  for (const trace of traces) {
    const id = trimId(trace.id)
    if (id) allowed.add(id)
  }
  const byRow = new Map<string, AgentTrace[]>()
  for (const rowId of plan.drawnRowIds) {
    const trace = plan.tree.nodeForRow(rowId)?.trace
    if (!trace) continue
    // 只画本层给的档案：本层的 head（帧自己的 trace）被 seed 进树是为了接父子轴，
    // 它的宿主行在**上一层** —— 在这里画它会让帧自引用（无限递归 → 栈溢出）。
    if (!allowed.has(trimId(trace.id))) continue
    byRow.set(rowId, [trace])
  }
  const detached: AgentTrace[] = []
  for (const trace of traces) {
    const rowId = trimId(trace.parentToolCallId)
    if (rowId && plan.drawnRowIds.has(rowId) && plan.tree.nodeForRow(rowId)) continue
    detached.push(trace)
  }
  return { byRow, detached }
}
