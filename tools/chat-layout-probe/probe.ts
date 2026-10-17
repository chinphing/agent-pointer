/**
 * Chat layout-cascade probe (dev-only, not part of the app bundle).
 *
 * Reproduces the box tree that carries the "composer height change → whole
 * transcript relayout" cascade, using the **real** stylesheet
 * (`src/styles/globals.css`), the **real** virtualizer library and the app's
 * own offset-observation module — so a keystroke here pays the same layout and
 * script costs the app pays, without booting the chat store.
 *
 * Box tree mirrored from ChatView.vue / MessageList.vue / Composer.vue:
 *
 *   div.flex-1.flex.flex-col.min-h-0                     (ChatView root)
 *     div.chat-main.flex-1.overflow-hidden.relative      (AppShell main pane)
 *       div.flex.h-full.min-h-0.flex-col
 *         div.relative.min-h-0.flex-1
 *           div.relative.h-full.min-h-0                  (MessageList root)
 *             div.chat-scroll-area.h-full.overflow-y-auto.chat-shell.pb-6   (scroller)
 *               div.flex.min-h-full.w-full.flex-col      (scroll content)
 *                 div.chat-column.shrink-0               (virtual column, height = totalSize)
 *                   div.chat-virtual-row.absolute…       (rows, contain: layout)
 *     div.chat-shell.shell-chat.shrink-0.pt-2.pb-5       (Composer footer)
 *       textarea.composer-textarea
 *
 * Query params:
 *   v=baseline|nominheight|containcontent|containscroller|overlay|overlay_scrollpad|overlay_nominheight
 *   ro=0|1            disable the app's scroller-ResizeObserver compensation
 *   fallback=1        force the JS auto-grow path (engines without field-sizing)
 *   rows=400          virtual row count
 *   words=60          words per row body
 */
import {
  computed,
  createApp,
  defineComponent,
  h,
  onBeforeUnmount,
  onMounted,
  ref,
  watch
} from 'vue'
import { useVirtualizer } from '@tanstack/vue-virtual'
import {
  MESSAGE_VIRTUAL_ROW_CLASS,
  messageVirtualizerBaseOptions
} from '../../src/lib/messageVirtualization'
import { createVirtualOffsetObservation } from '../../src/lib/virtualOffsetObservation'
import {
  createComposerTextareaResizeState,
  resizeComposerTextarea
} from '../../src/lib/composerTextareaResize'
import '../../src/styles/globals.css'

const params = new URLSearchParams(window.location.search)
const variant = params.get('v') ?? 'baseline'
const withRo = params.get('ro') !== '0'
const jsFallback = params.get('fallback') === '1'
const rowCount = Number(params.get('rows') ?? 400)
const wordsPerRow = Number(params.get('words') ?? 60)

const overlay = variant.startsWith('overlay')
const overlayScrollPadding = variant === 'overlay_scrollpad'
const dropMinHeight = variant === 'nominheight' || variant === 'overlay_nominheight'
const containContent = variant === 'containcontent'
const containScroller = variant === 'containscroller'
/**
 * `isolated`: the textarea stops being an in-flow flex item of the composer —
 * it is absolutely positioned inside a JS-height-managed, layout-contained box.
 * The composer's box still grows with the text (product behaviour), but a
 * keystroke's layout dirtiness no longer reaches the composer's ancestors
 * (pane → scroller → transcript), because an out-of-flow box's layout does not
 * propagate to its in-flow ancestors.
 */
const isolated = variant === 'isolated'
/**
 * `flexend`: the scroller becomes a flex column and the transcript content is
 * bottom-aligned with `margin-top: auto`, replacing the `min-h-full`
 * (percentage-height) wrapper + `flex-1` spacer. Product behaviour is the same
 * — short content is pushed to the bottom, long content scrolls — but the
 * transcript subtree no longer carries a *percentage height* on the scroller,
 * which is the only reason a scroller resize can invalidate it.
 */
const flexEnd = variant === 'flexend'
/**
 * `skipcomp=1` models the shipped cheap path: a scroll event that our own
 * viewport-shrink compensation caused does not schedule the full scroll pass
 * (MessageList's pass does DOM reads — forcing a synchronous layout right after
 * the compensation write — and then writes reactive state, dirtying layout
 * again before paint).
 */
const skipCompensationPass = params.get('skipcomp') === '1'

const rowText = Array.from({ length: wordsPerRow }, (_, i) => `word${i}`).join(' ')
const rowCode = Array.from(
  { length: 12 },
  (_, i) => `const value${i} = compute(${i}) // line ${i}`
).join('\n')

/**
 * A message row that is heavy enough for a transcript-wide relayout to be
 * visible in the layout counters: three text paragraphs (markdown-rendered
 * messages in the app), a fenced code block and a small table.
 */
function richRowBody() {
  return [
    h('div', { class: 'chat-column' }, [
      h('p', { class: 'text-[15px] leading-normal text-foreground' }, rowText),
      h('p', { class: 'text-[15px] leading-normal text-foreground' }, rowText),
      h('p', { class: 'text-[15px] leading-normal text-foreground' }, rowText),
      h('pre', { class: 'overflow-x-auto rounded-lg border border-border p-3 text-xs' }, [
        h('code', {}, rowCode)
      ]),
      h('table', { class: 'w-full border-collapse text-sm' }, [
        h('tbody', {}, [
          h('tr', {}, [h('td', {}, 'a'), h('td', {}, 'b'), h('td', {}, 'c')]),
          h('tr', {}, [h('td', {}, 'd'), h('td', {}, 'e'), h('td', {}, 'f')]),
          h('tr', {}, [h('td', {}, 'g'), h('td', {}, 'h'), h('td', {}, 'i')]),
          h('tr', {}, [h('td', {}, 'j'), h('td', {}, 'k'), h('td', {}, 'l')])
        ])
      ])
    ])
  ]
}

interface KeystrokeLog {
  seq: number
  textLength: number
  composerHeight: number
  scrollerClientHeight: number
  contentScrollHeight: number
}

/**
 * One entry per composer-resize frame — the pass `Composer.vue` marks as
 * `composerResize`. `forcedLayoutMs` is the layout that keystroke owes, flushed
 * by reading the scroller: at rAF time the browser has not run style/layout for
 * this frame yet, so this read pays exactly the cost the keystroke created
 * (composer box → pane → scroller → transcript). It is the same measurement the
 * render-perf HUD attributes to `composerResize` / `ms:forcedLayout`.
 */
interface ResizeFrameLog {
  seq: number
  resizeMs: number
  forcedLayoutMs: number
  composerHeight: number
  textareaHeight: number
  scrollerClientHeight: number
  contentScrollHeight: number
}

interface ProbeStats {
  scrollerResizeCallbacks: number
  compensations: number
  rangeChanges: number
  composerHeightChanges: number
  scrollPasses: number
  scrollEvents: number
}

const stats: ProbeStats = {
  scrollerResizeCallbacks: 0,
  compensations: 0,
  rangeChanges: 0,
  composerHeightChanges: 0
}

const Probe = defineComponent({
  name: 'ChatLayoutProbe',
  setup() {
    const scroller = ref<HTMLDivElement | null>(null)
    const contentEl = ref<HTMLDivElement | null>(null)
    const composerRoot = ref<HTMLDivElement | null>(null)
    const textarea = ref<HTMLTextAreaElement | null>(null)
    /** Composer outer height, mirrored into the transcript padding (overlay only). */
    const composerInset = ref(0)
    /** `isolated` only: the height of the box the out-of-flow textarea lives in. */
    const textareaBoxHeight = ref(36)
    /** Scroll-pass outputs (the app writes reactive state in the same shape). */
    const navVisibleRows = ref(0)
    const showScrollButton = ref(false)
    const keystrokes: KeystrokeLog[] = []
    const frameLog: ResizeFrameLog[] = []
    let inputSeq = 0
    let lastComposerHeight = 0
    let lastScrollerClientHeight = 0
    let scrollerResizeObserver: ResizeObserver | null = null
    let composerResizeObserver: ResizeObserver | null = null
    let isolatedResizeObserver: ResizeObserver | null = null
    let composerResizeRaf: number | null = null
    let composerResizeState = createComposerTextareaResizeState()

    const offsetObservation = createVirtualOffsetObservation()

    const virtualizer = useVirtualizer<HTMLDivElement, HTMLDivElement>(
      computed(() => ({
        ...messageVirtualizerBaseOptions(rowCount, index => `row-${index}`),
        getScrollElement: () => scroller.value,
        observeElementOffset: offsetObservation.observeElementOffset,
        measureElement: (el: Element) => el.getBoundingClientRect().height
      }))
    )

    const virtualRows = computed(() => virtualizer.value.getVirtualItems())
    const totalSize = computed(() => virtualizer.value.getTotalSize())

    watch(
      virtualRows,
      () => {
        stats.rangeChanges += 1
      },
      { flush: 'sync' }
    )

    /** Mounted row elements — the pass below reads their rects, like the app's. */
    const rowElements = new Set<HTMLElement>()
    function setRowElement(el: unknown) {
      const node =
        (el as { $el?: unknown } | null)?.$el instanceof HTMLElement
          ? (el as { $el: HTMLElement }).$el
          : el
      if (node instanceof HTMLElement) rowElements.add(node)
    }

    let lastScrollTop = 0
    let scrollPassRaf: number | null = null
    /** One-shot: the next scroll event is ours (viewport-shrink compensation). */
    let compensationScrollPending = false

    function scheduleScrollPass() {
      if (scrollPassRaf != null) return
      scrollPassRaf = requestAnimationFrame(() => {
        scrollPassRaf = null
        runScrollPass()
      })
    }

    /**
     * MessageList's `runScrollPassBody` in shape: every scroller / DOM read up
     * front (so a dirty tree is laid out synchronously here), then the reactive
     * writes, which dirty layout again before paint.
     */
    function runScrollPass() {
      const el = scroller.value
      if (!el) return
      stats.scrollPasses += 1
      const scrollTop = el.scrollTop
      const clientHeight = el.clientHeight
      const distance = el.scrollHeight - scrollTop - clientHeight
      let visible = 0
      for (const row of rowElements) {
        const rect = row.getBoundingClientRect()
        if (rect.bottom > 0 && rect.top < clientHeight) visible += 1
      }
      navVisibleRows.value = visible
      showScrollButton.value = distance > 120
      lastScrollTop = scrollTop
    }

    function onScroll() {
      stats.scrollEvents += 1
      if (skipCompensationPass && compensationScrollPending) {
        compensationScrollPending = false
        lastScrollTop = scroller.value?.scrollTop ?? lastScrollTop
        return
      }
      scheduleScrollPass()
    }

    /**
     * MessageList.vue:590 `applyScrollerViewportResize` — the app's reaction to a
     * scroller height change: compensate the viewport delta with a scroll write
     * and re-state the offset. Reproduced verbatim (without the search-locate /
     * total-size guards that need the chat store).
     */
    function applyScrollerViewportResize() {
      const el = scroller.value
      if (!el) return
      const previousHeight = lastScrollerClientHeight
      const nextHeight = el.clientHeight
      if (nextHeight === previousHeight) return
      lastScrollerClientHeight = nextHeight
      if (nextHeight > previousHeight) return
      const delta = previousHeight - nextHeight
      if (delta <= 0) return
      stats.compensations += 1
      if (skipCompensationPass) compensationScrollPending = true
      el.scrollTop += delta
      offsetObservation.sync()
    }

    function recordKeystroke() {
      const el = textarea.value
      const scrollerEl = scroller.value
      if (!el || !scrollerEl) return
      const composerHeight = composerRoot.value?.offsetHeight ?? 0
      if (composerHeight !== lastComposerHeight) {
        stats.composerHeightChanges += 1
        lastComposerHeight = composerHeight
      }
      keystrokes.push({
        seq: keystrokes.length,
        textLength: el.value.length,
        composerHeight,
        scrollerClientHeight: scrollerEl.clientHeight,
        contentScrollHeight: scrollerEl.scrollHeight
      })
    }

    /** Composer.vue `autoResize()` — the rAF pass that measures and re-sizes. */
    function autoResize() {
      if (composerResizeRaf != null) cancelAnimationFrame(composerResizeRaf)
      composerResizeRaf = requestAnimationFrame(() => {
        composerResizeRaf = null
        const el = textarea.value
        const scrollerEl = scroller.value
        if (!el || !scrollerEl) return
        const resizeStartedAt = performance.now()
        if (jsFallback) composerResizeState = resizeComposerTextarea(el, composerResizeState)
        const resizeMs = performance.now() - resizeStartedAt
        // Flush the layout this keystroke owes *here* — the frame the HUD names
        // `composerResize`. Nothing above has run style/layout yet for this frame,
        // so this read pays composer box → pane → scroller → transcript.
        const layoutStartedAt = performance.now()
        void scrollerEl.offsetHeight
        const forcedLayoutMs = performance.now() - layoutStartedAt
        frameLog.push({
          seq: inputSeq,
          resizeMs,
          forcedLayoutMs,
          composerHeight: composerRoot.value?.offsetHeight ?? 0,
          textareaHeight: el.offsetHeight,
          scrollerClientHeight: scrollerEl.clientHeight,
          contentScrollHeight: scrollerEl.scrollHeight
        })
      })
    }

    function onInput() {
      inputSeq += 1
      autoResize()
      recordKeystroke()
    }

    onMounted(() => {
      const el = textarea.value
      if (el && !jsFallback) {
        // `field-sizing: content` is declared by `.composer-textarea`; the app
        // only re-asserts the inline clamp once.
        el.style.setProperty('field-sizing', 'content')
        el.style.height = 'auto'
        el.style.minHeight = '36px'
        el.style.maxHeight = '250px'
        el.style.overflowY = 'auto'
      } else if (el) {
        // Simulate an engine without `field-sizing`: neutralise the class rule so
        // the JS auto-grow path owns the height.
        el.style.setProperty('field-sizing', 'auto')
      }

      const scrollerEl = scroller.value
      if (scrollerEl) {
        lastScrollerClientHeight = scrollerEl.clientHeight
        if (withRo && typeof ResizeObserver !== 'undefined') {
          scrollerResizeObserver = new ResizeObserver(() => {
            stats.scrollerResizeCallbacks += 1
            applyScrollerViewportResize()
            requestAnimationFrame(() => applyScrollerViewportResize())
          })
          scrollerResizeObserver.observe(scrollerEl)
        }
      }

      if (isolated && typeof ResizeObserver !== 'undefined') {
        // Mirror the out-of-flow textarea's height onto its in-flow box: the
        // composer still grows with the text, one write per *change*.
        const node = textarea.value
        if (node) {
          textareaBoxHeight.value = node.offsetHeight
          const observer = new ResizeObserver(() => {
            const el = textarea.value
            if (!el) return
            const next = el.offsetHeight
            if (next === textareaBoxHeight.value) return
            textareaBoxHeight.value = next
          })
          observer.observe(node)
          isolatedResizeObserver = observer
        }
      }

      if (overlay && typeof ResizeObserver !== 'undefined') {
        const composerEl = composerRoot.value
        if (composerEl) {
          lastComposerHeight = composerEl.offsetHeight
          composerInset.value = composerEl.offsetHeight
          composerResizeObserver = new ResizeObserver(() => {
            const node = composerRoot.value
            if (!node) return
            const next = node.offsetHeight
            if (next === composerInset.value) return
            composerInset.value = next
          })
          composerResizeObserver.observe(composerEl)
        }
      }

      // Long conversation, parked at the bottom (the state the report is about).
      requestAnimationFrame(() => {
        const node = scroller.value
        if (node) node.scrollTop = node.scrollHeight - node.clientHeight
        requestAnimationFrame(() => {
          probe.ready = true
        })
      })
    })

    onBeforeUnmount(() => {
      scrollerResizeObserver?.disconnect()
      composerResizeObserver?.disconnect()
      if (composerResizeRaf != null) cancelAnimationFrame(composerResizeRaf)
    })

    const probe = {
      ready: false,
      variant,
      withRo,
      jsFallback,
      stats,
      keystrokes,
      frameLog,
      reset() {
        keystrokes.length = 0
        frameLog.length = 0
        inputSeq = 0
        stats.scrollerResizeCallbacks = 0
        stats.compensations = 0
        stats.rangeChanges = 0
        stats.composerHeightChanges = 0
        stats.scrollPasses = 0
        stats.scrollEvents = 0
      },
      snapshot() {
        const el = textarea.value
        const scrollerEl = scroller.value
        return {
          text: el?.value ?? '',
          textLength: el?.value.length ?? 0,
          textareaHeight: el?.offsetHeight ?? 0,
          composerHeight: composerRoot.value?.offsetHeight ?? 0,
          scrollerClientHeight: scrollerEl?.clientHeight ?? 0,
          scrollerScrollHeight: scrollerEl?.scrollHeight ?? 0,
          scrollerScrollTop: scrollerEl?.scrollTop ?? 0,
          totalSize: totalSize.value,
          renderedRows: virtualRows.value.length,
          composerInset: composerInset.value,
          /**
           * Distance between the scroller's bottom edge and the transcript
           * content's bottom edge. The product contract: when the content is
           * shorter than the viewport it must be pushed to the bottom (0), and
           * when it overflows it must be negative by exactly the scroll extent.
           */
          contentBottomGap: (() => {
            const s = scroller.value
            const c = contentEl.value
            if (!s || !c) return null
            return (
              Math.round(
                (s.getBoundingClientRect().bottom - c.getBoundingClientRect().bottom) * 100
              ) / 100
            )
          })()
        }
      },
      setText(text: string) {
        const el = textarea.value
        if (!el) return
        el.value = text
        el.dispatchEvent(new Event('input', { bubbles: true }))
      },
      focusTextarea() {
        textarea.value?.focus()
      }
    }
    ;(window as unknown as { __probe: typeof probe }).__probe = probe

    const contentClass = dropMinHeight || flexEnd
      ? 'flex w-full flex-col'
      : 'flex min-h-full w-full flex-col'
    const contentStyle = computed<Record<string, string>>(() => {
      const style: Record<string, string> = {}
      if (containContent) style.contain = 'layout'
      if (flexEnd) {
        // Bottom-align when the content is shorter than the viewport — the
        // `min-h-full` + `flex-1` spacer behaviour, without a percentage height.
        style.marginTop = 'auto'
        style.flex = '0 0 auto'
      }
      if (overlay && !overlayScrollPadding && composerInset.value > 0) {
        style.paddingBottom = `${composerInset.value}px`
      }
      return style
    })

    const scrollerStyle = computed<Record<string, string>>(() => {
      const style: Record<string, string> = { overflowAnchor: 'none' }
      if (containScroller) style.contain = 'layout'
      if (flexEnd) {
        style.display = 'flex'
        style.flexDirection = 'column'
      }
      if (overlay && overlayScrollPadding && composerInset.value > 0) {
        style.paddingBottom = `calc(1.5rem + ${composerInset.value}px)`
      }
      return style
    })

    return () => {
      const rows = virtualRows.value.map(virtualRow =>
        h(
          'div',
          {
            key: String(virtualRow.key),
            'data-index': virtualRow.index,
            class: ['absolute left-0 top-0 w-full', MESSAGE_VIRTUAL_ROW_CLASS],
            ref: setRowElement,
            style: {
              transform: `translateY(${virtualRow.start}px)`,
              paddingTop: `${virtualRow.index === 0 ? 0 : 28}px`
            }
          },
          [
            h('div', { class: 'chat-column' }, [richRowBody()])
          ]
        )
      )

      const textareaNode = h('textarea', {
        ref: textarea,
        rows: 1,
        class:
          'composer-textarea block min-w-0 flex-1 resize-none overflow-x-hidden overflow-y-auto bg-transparent border-0 outline-none px-2 py-2 text-[15px] leading-5 text-foreground placeholder:text-muted md:w-full md:flex-none md:px-3 md:pt-[3px] md:pb-2 md:leading-normal',
        style: {
          maxHeight: '250px',
          ...(isolated
            ? { position: 'absolute', left: '0', right: '0', top: '0', width: '100%' }
            : {})
        },
        onInput
      })

      const composer = h(
        'div',
        {
          ref: composerRoot,
          class: 'chat-shell shell-chat shrink-0 pt-2 pb-5',
          style: overlay
            ? { position: 'absolute', left: '0', right: '0', bottom: '0', zIndex: '20' }
            : undefined
        },
        [
          h('div', { class: 'chat-column' }, [
            h('div', { class: 'composer-shell rounded-2xl overflow-visible px-2' }, [
              h(
                'div',
                { class: 'composer-body flex items-end gap-1 md:flex-col md:items-stretch md:gap-0' },
                [
                  isolated
                    ? h(
                        'div',
                        {
                          class: 'relative w-full',
                          style: {
                            height: `${textareaBoxHeight.value}px`,
                            contain: 'layout'
                          }
                        },
                        [textareaNode]
                      )
                    : textareaNode
                ]
              )
            ])
          ])
        ]
      )

      return h(
        'div',
        {
          class: 'flex-1 flex flex-col min-h-0',
          style: { height: '100%', position: overlay ? 'relative' : undefined }
        },
        [
          h('div', { class: 'chat-main flex-1 overflow-hidden relative' }, [
            h('div', { class: 'flex h-full min-h-0 flex-col' }, [
              h('div', { class: 'relative min-h-0 flex-1' }, [
                h('div', { class: 'relative h-full min-h-0' }, [
                  h(
                    'div',
                    {
                      ref: scroller,
                      class:
                        'chat-scroll-area scrollbar-hide h-full overflow-y-auto chat-shell pb-6',
                      style: scrollerStyle.value,
                      onScroll
                    },
                    [
                      h('div', { ref: contentEl, class: contentClass, style: contentStyle.value }, [
                        h('div', {
                          class: 'flex shrink-0 items-center justify-center overflow-hidden',
                          style: { height: '0px' }
                        }),
                        h(
                          'div',
                          {
                            class: 'chat-column relative w-full shrink-0',
                            style: { height: `${totalSize.value}px` }
                          },
                          rows
                        )
                      ])
                    ]
                  )
                ])
              ])
            ])
          ]),
          composer
        ]
      )
    }
  }
})

createApp(Probe).mount('#app')
