#!/usr/bin/env node
/**
 * Chat layout-cascade probe driver (dev-only).
 *
 * Drives `tools/chat-layout-probe/` in a real Chromium over CDP and records, per
 * keystroke:
 *
 *   - the engine's layout / style counters (`Performance.getMetrics`), and
 *   - the `Layout` events from a `devtools.timeline` trace, whose
 *     `beginData.dirtyObjects` / `totalObjects` say *how much of the tree* one
 *     layout pass had to visit — the "whole transcript relayout" number,
 *   - the probe's own state (composer height, scroller client height).
 *
 * Phases (each keystroke traced on its own, so the cost is attributed exactly):
 *   soft    — type characters; the composer grows only at soft-wrap boundaries
 *   grow    — insert a newline per step: the composer height changes every step
 *   delete  — backspace; the composer shrinks only at soft-wrap boundaries
 *
 * Usage:
 *   npm run dev            # in another shell
 *   node tools/chat-layout-probe/run.mjs --variant baseline
 *   node tools/chat-layout-probe/run.mjs --variant nominheight --out /tmp/x.json
 */
import { spawn } from 'node:child_process'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const CHROME =
  process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'

function arg(name, fallback) {
  const hit = process.argv.find(a => a.startsWith(`--${name}=`))
  if (hit) return hit.slice(name.length + 3)
  const idx = process.argv.indexOf(`--${name}`)
  if (idx >= 0 && process.argv[idx + 1] && !process.argv[idx + 1].startsWith('--')) {
    return process.argv[idx + 1]
  }
  return process.argv.indexOf(`--${name}`) >= 0 ? '1' : fallback
}

const BASE = arg('base', 'http://127.0.0.1:1420')
const VARIANT = arg('variant', 'baseline')
const RO = arg('ro', '1')
const FALLBACK = arg('fallback', '0')
const SKIPCOMP = arg('skipcomp', '0')
const ROWS = arg('rows', '400')
const WORDS = arg('words', '80')
const SOFT = Number(arg('soft', '200'))
const GROW = Number(arg('grow', '20'))
const PULSES = Number(arg('pulses', '30'))
const DELETES = Number(arg('deletes', '40'))
const SETTLE = Number(arg('settle', '40'))
const PORT = Number(arg('port', '9333'))
const OUT = arg('out', '')
const WIDTH = Number(arg('width', '800'))
const HEIGHT = Number(arg('height', '900'))
const DUMP = arg('dump', '0') === '1'

const url =
  `${BASE}/tools/chat-layout-probe/index.html` +
  `?v=${VARIANT}&ro=${RO}&fallback=${FALLBACK}&rows=${ROWS}&words=${WORDS}&skipcomp=${SKIPCOMP}`

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

/* ------------------------------------------------------------------ CDP ---- */

class Cdp {
  constructor(ws) {
    this.ws = ws
    this.id = 0
    this.pending = new Map()
    this.handlers = new Map()
    ws.addEventListener('message', event => {
      const msg = JSON.parse(event.data)
      if (msg.id != null) {
        const entry = this.pending.get(msg.id)
        if (!entry) return
        this.pending.delete(msg.id)
        if (msg.error) entry.reject(new Error(JSON.stringify(msg.error)))
        else entry.resolve(msg.result)
        return
      }
      const list = this.handlers.get(msg.method)
      if (list) for (const fn of [...list]) fn(msg.params, msg.sessionId)
    })
  }

  on(method, fn) {
    const list = this.handlers.get(method) ?? []
    list.push(fn)
    this.handlers.set(method, list)
    return () => {
      this.handlers.set(
        method,
        (this.handlers.get(method) ?? []).filter(f => f !== fn)
      )
    }
  }

  once(method) {
    return new Promise(resolve => {
      const off = this.on(method, params => {
        off()
        resolve(params)
      })
    })
  }

  send(method, params = {}, sessionId) {
    this.id += 1
    const id = this.id
    const payload = { id, method, params }
    if (sessionId) payload.sessionId = sessionId
    this.ws.send(JSON.stringify(payload))
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject })
      setTimeout(() => {
        if (this.pending.delete(id)) reject(new Error(`CDP timeout: ${method}`))
      }, 60000)
    })
  }
}

async function waitForDevtools(port, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/version`)
      if (res.ok) return (await res.json()).webSocketDebuggerUrl
    } catch {
      /* not up yet */
    }
    await sleep(200)
  }
  throw new Error('Chrome DevTools endpoint never came up')
}

const METRIC_KEYS = [
  'LayoutCount',
  'LayoutDuration',
  'RecalcStyleCount',
  'RecalcStyleDuration',
  'ScriptDuration',
  'TaskDuration'
]

/* ---------------------------------------------------------------- main ---- */

async function main() {
  const userDataDir = mkdtempSync(join(tmpdir(), 'chat-layout-probe-'))
  const chrome = spawn(
    CHROME,
    [
      '--headless=new',
      `--remote-debugging-port=${PORT}`,
      `--user-data-dir=${userDataDir}`,
      `--window-size=${WIDTH},${HEIGHT}`,
      '--force-device-scale-factor=1',
      '--no-first-run',
      '--no-default-browser-check',
      '--disable-extensions',
      'about:blank'
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] }
  )
  chrome.stderr.on('data', () => {})

  let exitCode = 0
  try {
    const wsUrl = await waitForDevtools(PORT)
    const ws = new WebSocket(wsUrl)
    await new Promise((resolve, reject) => {
      ws.addEventListener('open', resolve, { once: true })
      ws.addEventListener('error', reject, { once: true })
    })
    const cdp = new Cdp(ws)

    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' })
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true })

    await cdp.send('Page.enable', {}, sessionId)
    await cdp.send('Runtime.enable', {}, sessionId)
    await cdp.send('Performance.enable', {}, sessionId)

    const loaded = cdp.once('Page.loadEventFired')
    await cdp.send('Page.navigate', { url }, sessionId)
    await loaded

    const deadline = Date.now() + 20000
    for (;;) {
      const { result } = await cdp.send(
        'Runtime.evaluate',
        { expression: 'Boolean(window.__probe && window.__probe.ready)', returnByValue: true },
        sessionId
      )
      if (result.value) break
      if (Date.now() > deadline) throw new Error('probe never became ready')
      await sleep(100)
    }

    await cdp.send('Runtime.evaluate', { expression: 'window.__probe.focusTextarea()' }, sessionId)
    await sleep(200)

    const evaluate = async expression => {
      const { result } = await cdp.send(
        'Runtime.evaluate',
        { expression, returnByValue: true },
        sessionId
      )
      return result.value
    }

    const readMetrics = async () => {
      const { metrics } = await cdp.send('Performance.getMetrics', {}, sessionId)
      const out = {}
      for (const { name, value } of metrics) out[name] = value
      return out
    }
    const diffMetrics = (before, after) => {
      const out = {}
      for (const key of METRIC_KEYS) out[key] = (after[key] ?? 0) - (before[key] ?? 0)
      return out
    }

    /**
     * One keystroke: engine metrics around it, probe state after it. (A
     * `devtools.timeline` trace would give `dirtyObjects` per layout pass, but
     * this Chrome build exposes `Tracing.start` without `Tracing.stop` over CDP,
     * so the driver uses `Performance.getMetrics` — the engine's own accounting.)
     */
    async function tracedStep(dispatch) {
      await dispatch()
      await sleep(SETTLE)
      return []
    }

    function summarizeTrace(events) {
      let layoutCount = 0
      let dirtySum = 0
      let dirtyMax = 0
      let objectsMax = 0
      let durationUs = 0
      let styleCount = 0
      for (const event of events) {
        if (event.name === 'UpdateLayoutTree') {
          styleCount += 1
          continue
        }
        layoutCount += 1
        const begin = event.args?.beginData ?? {}
        const dirty = Number(begin.dirtyObjects ?? 0)
        const total = Number(begin.totalObjects ?? 0)
        if (Number.isFinite(dirty)) {
          dirtySum += dirty
          if (dirty > dirtyMax) dirtyMax = dirty
        }
        if (Number.isFinite(total) && total > objectsMax) objectsMax = total
        durationUs += Number(event.dur ?? 0)
      }
      return {
        layoutEvents: layoutCount,
        styleEvents: styleCount,
        dirtySum,
        dirtyMax,
        objectsMax,
        traceLayoutMs: durationUs / 1000
      }
    }

    const records = []
    let dumped = 0

    async function step(kind, index, dispatch) {
      const before = await readMetrics()
      const events = await tracedStep(dispatch)
      const after = await readMetrics()
      const snapshot = await evaluate('window.__probe.snapshot()')
      const frame = await evaluate(
        '(() => { const log = window.__probe.frameLog; return log.length ? log[log.length - 1] : null })()'
      )
      records.push({
        kind,
        index,
        snapshot,
        frame,
        forcedLayoutMs: frame?.forcedLayoutMs ?? 0,
        resizeMs: frame?.resizeMs ?? 0,
        ...diffMetrics(before, after)
      })
    }

    // Warm-up (JIT, fonts, first paint) — not measured.
    for (let i = 0; i < 8; i += 1) {
      await cdp.send('Input.insertText', { text: 'w' }, sessionId)
      await sleep(SETTLE)
    }
    await evaluate('window.__probe.setText("")')
    await sleep(200)
    await evaluate('window.__probe.reset()')

    const pressBackspace = async () => {
      await cdp.send(
        'Input.dispatchKeyEvent',
        {
          type: 'rawKeyDown',
          key: 'Backspace',
          code: 'Backspace',
          windowsVirtualKeyCode: 8,
          nativeVirtualKeyCode: 8
        },
        sessionId
      )
      await cdp.send(
        'Input.dispatchKeyEvent',
        {
          type: 'keyUp',
          key: 'Backspace',
          code: 'Backspace',
          windowsVirtualKeyCode: 8,
          nativeVirtualKeyCode: 8
        },
        sessionId
      )
    }

    // 1) Soft-wrap typing: height changes only when a line wraps.
    for (let i = 0; i < SOFT; i += 1) {
      await step('soft', i, () => cdp.send('Input.insertText', { text: 'm' }, sessionId))
    }
    // 2) Deterministic growth: every step adds a line to the composer.
    for (let i = 0; i < GROW; i += 1) {
      await step('grow', i, () => cdp.send('Input.insertText', { text: '\n' }, sessionId))
    }
    // 3) Pulse: newline + backspace — two height-changing keystrokes per pair,
    //    which is what a user deleting characters through a wrap boundary does.
    for (let i = 0; i < PULSES; i += 1) {
      await step('pulse', i, () => cdp.send('Input.insertText', { text: '\n' }, sessionId))
      await step('pulse', i, pressBackspace)
    }
    // 4) Deletion (the reported symptom) — height changes only at wrap boundaries.
    for (let i = 0; i < DELETES; i += 1) {
      await step('delete', i, pressBackspace)
    }

    const finalSnapshot = await evaluate('window.__probe.snapshot()')
    const probeStats = await evaluate('JSON.parse(JSON.stringify(window.__probe.stats))')

    /**
     * Calibration: the cost of one *unambiguously* transcript-wide relayout, for
     * scale. Nudging the scroller width re-wraps every mounted row, so the number
     * below is "what laying out the whole transcript costs" on this box tree —
     * the yardstick a composer-height keystroke has to be compared against.
     */
    const calibrationBefore = await readMetrics()
    await evaluate(
      "(() => { const el = document.querySelector('.chat-scroll-area'); if (!el) return false; el.style.width = 'calc(100% - 1px)'; return true })()"
    )
    await sleep(400)
    const calibrationAfter = await readMetrics()
    const calibration = {
      ...diffMetrics(calibrationBefore, calibrationAfter),
      layoutMs: (calibrationAfter.LayoutDuration - calibrationBefore.LayoutDuration) * 1000,
      taskMs: (calibrationAfter.TaskDuration - calibrationBefore.TaskDuration) * 1000
    }

    /* ------------------------------------------------------- aggregate ---- */

    function aggregate(kind) {
      const rows = records.filter(r => r.kind === kind)
      const changed = []
      const steady = []
      let previousHeight = null
      for (const row of rows) {
        const height = row.snapshot.composerHeight
        const isChange = previousHeight !== null && height !== previousHeight
        previousHeight = height
        ;(isChange ? changed : steady).push(row)
      }
      const mean = (list, key) =>
        list.length ? list.reduce((sum, r) => sum + r[key], 0) / list.length : 0
      const summarize = list => ({
        n: list.length,
        forcedLayoutMs: mean(list, 'forcedLayoutMs'),
        forcedLayoutMsMax: list.length ? Math.max(...list.map(r => r.forcedLayoutMs)) : 0,
        resizeMs: mean(list, 'resizeMs'),
        layoutCount: mean(list, 'LayoutCount'),
        layoutMs: mean(list, 'LayoutDuration') * 1000,
        layoutCountMax: list.length ? Math.max(...list.map(r => r.LayoutCount)) : 0,
        recalcStyleCount: mean(list, 'RecalcStyleCount'),
        recalcStyleMs: mean(list, 'RecalcStyleDuration') * 1000,
        scriptMs: mean(list, 'ScriptDuration') * 1000,
        taskMs: mean(list, 'TaskDuration') * 1000,
        taskMsMax: list.length ? Math.max(...list.map(r => r.TaskDuration)) * 1000 : 0
      })
      return {
        all: summarize(rows),
        heightChanged: summarize(changed),
        heightSteady: summarize(steady),
        heightChangeCount: changed.length
      }
    }

    const summary = {
      variant: VARIANT,
      ro: RO,
      fallback: FALLBACK,
      rows: Number(ROWS),
      words: Number(WORDS),
      soft: SOFT,
      grow: GROW,
      deletes: DELETES,
      settleMs: SETTLE,
      url,
      probeStats,
      finalSnapshot,
      softPhase: aggregate('soft'),
      growPhase: aggregate('grow'),
      deletePhase: aggregate('delete'),
      calibration
    }

    const pad = (value, width = 9) => String(value).padStart(width)
    const header =
      `${''.padEnd(24)} ${'n'.padStart(4)} ${'forcedMs'.padStart(9)} ${'frcMax'.padStart(7)} ` +
      `${'resizeMs'.padStart(9)} ${'layCnt'.padStart(7)} ${'layMs'.padStart(7)} ` +
      `${'recalc'.padStart(7)} ${'scriptMs'.padStart(8)} ${'taskMs'.padStart(7)} ${'taskMax'.padStart(8)}`
    const line = (label, agg) =>
      [
        label.padEnd(24),
        pad(agg.n, 4),
        pad(agg.forcedLayoutMs.toFixed(3), 9),
        pad(agg.forcedLayoutMsMax.toFixed(2), 7),
        pad(agg.resizeMs.toFixed(3), 9),
        pad(agg.layoutCount.toFixed(2), 7),
        pad(agg.layoutMs.toFixed(2), 7),
        pad(agg.recalcStyleCount.toFixed(2), 7),
        pad(agg.scriptMs.toFixed(2), 8),
        pad(agg.taskMs.toFixed(2), 7),
        pad(agg.taskMsMax.toFixed(2), 8)
      ].join(' ')

    console.log(`variant=${VARIANT} ro=${RO} fallback=${FALLBACK} ${url}`)
    console.log(header)
    console.log(line('soft / all', summary.softPhase.all))
    console.log(line('soft / height changed', summary.softPhase.heightChanged))
    console.log(line('soft / height steady', summary.softPhase.heightSteady))
    console.log(line('grow / all', summary.growPhase.all))
    console.log(line('delete / all', summary.deletePhase.all))
    console.log(line('delete / height changed', summary.deletePhase.heightChanged))
    console.log(line('delete / height steady', summary.deletePhase.heightSteady))
    console.log(
      `height-change keystrokes: soft ${summary.softPhase.heightChangeCount}, ` +
        `grow ${summary.growPhase.heightChangeCount}, delete ${summary.deletePhase.heightChangeCount}`
    )
    console.log(`probe stats: ${JSON.stringify(probeStats)}`)
    console.log(`final: ${JSON.stringify(finalSnapshot)}`)

    if (OUT) {
      writeFileSync(OUT, `${JSON.stringify({ summary, records }, null, 2)}\n`)
      console.log(`wrote ${OUT}`)
    }

    ws.close()
  } catch (error) {
    console.error(String(error && error.stack ? error.stack : error))
    exitCode = 1
  } finally {
    chrome.kill('SIGTERM')
  }
  process.exit(exitCode)
}

void main()
