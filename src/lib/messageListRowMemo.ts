/**
 * Dependency vector for the row-level `v-memo` on the virtual message rows
 * (`MessageList.vue`).
 *
 * ## Why it sits on the geometry wrapper
 *
 * `v-memo` **inside** a `v-for` compiles to `withMemo(..., _cache, <fixed index>)`,
 * i.e. one cache slot shared by every iteration. Measured consequences (see
 * `messageListRowMemo.test.ts`): with per-row-unique deps it never hits (a no-op);
 * with deps that are equal across rows it hands one row's vnode to another
 * (the DOM showed `["a","a"]`). The only form the compiler optimizes is `v-memo`
 * **on the same element as `v-for`** — here, the row wrapper that also carries
 * `:ref` / `:data-index` / `translateY`.
 *
 * That is safe **only because the geometry inputs are part of this vector**: a memo
 * hit then means `start` and `index` are unchanged, so the wrapper's `transform`,
 * `data-index` and measure registration have nothing to update; any geometry change
 * (scroll, row inserted above) forces a miss and the wrapper is patched normally.
 *
 * ## Completeness is the whole game
 *
 * A missing input is a **silent visual freeze**, not a perf bug. Every field below is
 * covered by `messageListRowMemo.test.ts` (flip one input → the vector must differ),
 * and the test also pins the field list, so adding an input without adding it to the
 * vector fails the suite.
 */
export type MessageRowMemoInput = {
  /** Virtualizer row offset — part of the wrapper's `translateY`. */
  start: number
  /** Virtualizer row index — `data-index`, and the measure batch key. */
  index: number
  /**
   * Turn object identity. `buildMessageListLayout` hands out **new** turn objects on
   * every rebuild (`rebindTurn`), so this one value covers `entries`,
   * `collapsedEntries`, `state` and `hiddenCount`.
   */
  turn: unknown
  /** Collapsed vs expanded projection (`displayedTurnEntries`). */
  expanded: boolean
  /** 「工作」chip visible for this turn. */
  showElapsed: boolean
  /** Elapsed label — moves once per second while the turn is running. */
  elapsedLabel: string
  /** `turnFileChanges.get(turn.id)` — drives the ChangeSummary block. */
  files: unknown
  /** ChangeSummary expanded / collapsed. */
  changeSummaryExpanded: boolean
  /** `TurnSpawnPlan` identity for this turn (sub-agent frame placement). */
  spawnPlan: unknown
  /** Search / focus highlight inputs read by `entryIs*` / `groupsHighlightClass`. */
  searchMatchIds: unknown
  activeSearchMessageId: unknown
  activeSearchToolCallId: unknown
  focusHighlightMessageId: unknown
  /** Task board chrome: `entry === activeBoard`, sticky shell, panel props. */
  activeBoard: unknown
  activeBoardSticky: boolean
  conversationId: unknown
  taskBoards: unknown
  /** `computerAnnotatedScreenViewEnabled` — the only setting `shouldShowGlueMessage` reads. */
  glueEnabled: boolean
}

export function messageRowMemoDeps(input: MessageRowMemoInput): unknown[] {
  return [
    input.start,
    input.index,
    input.turn,
    input.expanded,
    input.showElapsed,
    input.elapsedLabel,
    input.files,
    input.changeSummaryExpanded,
    input.spawnPlan,
    input.searchMatchIds,
    input.activeSearchMessageId,
    input.activeSearchToolCallId,
    input.focusHighlightMessageId,
    input.activeBoard,
    input.activeBoardSticky,
    input.conversationId,
    input.taskBoards,
    input.glueEnabled
  ]
}
