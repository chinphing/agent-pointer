import { getActivePinia } from 'pinia'
import { useSettingsStore } from '../stores/settings'

type WindowWithWebkitAudio = Window & {
  webkitAudioContext?: typeof AudioContext
}

let sharedCtx: AudioContext | null = null
/** Avoid double chime when two Done events race (e.g. reconnect replay). */
let lastPlayAtMs = 0
const PLAY_DEBOUNCE_MS = 800
let visibilityHooked = false

function resolveAudioContext(): AudioContext | null {
  if (typeof window === 'undefined') return null
  const w = window as WindowWithWebkitAudio
  const Ctor = window.AudioContext || w.webkitAudioContext
  if (!Ctor) {
    console.warn('[sound] AudioContext unavailable on this platform')
    return null
  }
  // Browsers may close the context after long idle / tab discard; recreate.
  if (sharedCtx && sharedCtx.state === 'closed') {
    console.warn('[sound] AudioContext was closed; recreating')
    sharedCtx = null
  }
  if (!sharedCtx) {
    sharedCtx = new Ctor()
  }
  return sharedCtx
}

function hookAudioContextVisibility(): void {
  if (visibilityHooked || typeof document === 'undefined') return
  visibilityHooked = true
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState !== 'visible') return
    const ctx = sharedCtx
    if (!ctx || ctx.state !== 'suspended') return
    void ctx.resume().then(
      () => console.info('[sound] AudioContext resumed on visibility'),
      err => console.warn('[sound] AudioContext resume on visibility failed', err)
    )
  })
}

/**
 * Unlock Web Audio during a user gesture (send / settings toggle).
 * WKWebView often keeps AudioContext suspended until resume() runs in-gesture;
 * Done arrives later without a gesture, so prime here or the chime is silent.
 */
export function primeTaskCompleteAudio(): void {
  hookAudioContextVisibility()
  const ctx = resolveAudioContext()
  if (!ctx) return
  if (ctx.state === 'suspended' || (ctx.state as string) === 'interrupted') {
    void ctx.resume().then(
      () => console.info('[sound] AudioContext primed (resumed) state=%s', ctx.state),
      err => console.warn('[sound] AudioContext prime failed', err)
    )
  }
}

type ToneLayer = {
  freq: number
  type: OscillatorType
  gain: number
  start: number
  attack: number
  dur: number
}

function playWithContext(ctx: AudioContext): void {
  const now = ctx.currentTime
  // Soft low-pass so residual triangle harmonics stay rounded.
  const filter = ctx.createBiquadFilter()
  filter.type = 'lowpass'
  filter.frequency.value = 1400
  filter.Q.value = 0.7
  const master = ctx.createGain()
  // Slight master lift: layer peaks stay moderate; perceived level was soft on laptops.
  master.gain.value = 1.45
  filter.connect(master)
  master.connect(ctx.destination)

  const layers: ToneLayer[] = [
    // Note 1 — G3 body + G4 presence
    { freq: 196.0, type: 'sine', gain: 0.26, start: 0, attack: 0.016, dur: 0.3 },
    { freq: 392.0, type: 'triangle', gain: 0.2, start: 0, attack: 0.012, dur: 0.26 },
    // Note 2 — D4 body + D5 soft lift (interval reads as "done")
    { freq: 293.66, type: 'sine', gain: 0.28, start: 0.11, attack: 0.018, dur: 0.38 },
    { freq: 587.33, type: 'triangle', gain: 0.14, start: 0.11, attack: 0.014, dur: 0.32 }
  ]

  for (const layer of layers) {
    const osc = ctx.createOscillator()
    const gain = ctx.createGain()
    osc.type = layer.type
    osc.frequency.value = layer.freq
    const t0 = now + layer.start
    gain.gain.setValueAtTime(0.0001, t0)
    gain.gain.exponentialRampToValueAtTime(layer.gain, t0 + layer.attack)
    gain.gain.exponentialRampToValueAtTime(layer.gain * 0.65, t0 + layer.attack + 0.07)
    gain.gain.exponentialRampToValueAtTime(0.0001, t0 + layer.dur)
    osc.connect(gain)
    gain.connect(filter)
    osc.start(t0)
    osc.stop(t0 + layer.dur + 0.03)
  }
  console.info('[sound] played task-complete chime')
}

/**
 * Mid-warm two-note chime: clear on laptop speakers, not shrill.
 * Body ~G3/D4, presence ~G4/D5 (kept under ~600Hz fundamentals).
 */
export async function playTaskCompleteSound(): Promise<void> {
  const nowMs = Date.now()
  if (nowMs - lastPlayAtMs < PLAY_DEBOUNCE_MS) {
    console.info('[sound] skip task-complete chime (debounced)')
    return
  }
  lastPlayAtMs = nowMs

  hookAudioContextVisibility()
  let ctx = resolveAudioContext()
  if (!ctx) return
  try {
    if (ctx.state === 'suspended' || (ctx.state as string) === 'interrupted') {
      await ctx.resume()
    }
    if (ctx.state === 'closed') {
      sharedCtx = null
      ctx = resolveAudioContext()
      if (!ctx) {
        console.warn('[sound] AudioContext closed and recreate failed')
        return
      }
      await ctx.resume().catch(err =>
        console.warn('[sound] AudioContext resume after recreate failed', err)
      )
    }
    if (ctx.state !== 'running') {
      console.warn('[sound] AudioContext not running after resume; state=%s', ctx.state)
      return
    }
    playWithContext(ctx)
  } catch (err) {
    console.warn('[sound] failed to play task-complete chime', err)
  }
}

/** Honor user preference; no-op when Pinia is unavailable (unit tests). */
export function playTaskCompleteSoundIfEnabled(): void {
  if (!getActivePinia()) return
  const settings = useSettingsStore()
  if (settings.userSettings.playSoundOnFinish === false) {
    console.info('[sound] skip task-complete chime (disabled in settings)')
    return
  }
  void playTaskCompleteSound()
}
