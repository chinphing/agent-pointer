import { invoke } from '@tauri-apps/api/core'
import { getActivePinia } from 'pinia'
import { useSettingsStore } from '../stores/settings'
import { isTauriRuntime } from './runtime'

/** Avoid double chime when two Done events race (e.g. reconnect replay). */
let lastPlayAtMs = 0
const PLAY_DEBOUNCE_MS = 800

/** Web-only HTMLAudio unlock / keep-warm (desktop uses native OS playback). */
let webAudioEl: HTMLAudioElement | null = null
let webObjectUrl: string | null = null
let webKeepWarmTimer: ReturnType<typeof setInterval> | null = null
const WEB_KEEP_WARM_MS = 12_000

function synthesizeChimeWav(): Blob {
  // Match original Web Audio graph: 4 layers, lowpass 1400Hz, master 1.45.
  const sampleRate = 44100
  const masterGain = 1.45
  const totalSecs = 0.55
  const n = Math.floor(sampleRate * totalSecs)
  const mixed = new Float32Array(n)
  const layers: Array<{
    freq: number
    wave: 'sine' | 'triangle'
    gain: number
    start: number
    attack: number
    dur: number
  }> = [
    { freq: 196.0, wave: 'sine', gain: 0.26, start: 0, attack: 0.016, dur: 0.3 },
    { freq: 392.0, wave: 'triangle', gain: 0.2, start: 0, attack: 0.012, dur: 0.26 },
    { freq: 293.66, wave: 'sine', gain: 0.28, start: 0.11, attack: 0.018, dur: 0.38 },
    { freq: 587.33, wave: 'triangle', gain: 0.14, start: 0.11, attack: 0.014, dur: 0.32 }
  ]

  const expRamp = (t: number, t0: number, v0: number, t1: number, v1: number) => {
    if (t <= t0) return v0
    if (t >= t1) return v1
    return v0 * (v1 / v0) ** ((t - t0) / (t1 - t0))
  }

  const layerGainAt = (
    layer: (typeof layers)[number],
    t: number
  ): number => {
    const t0 = layer.start
    const tAtt = t0 + layer.attack
    const tMid = tAtt + 0.07
    const tEnd = t0 + layer.dur
    const floor = 0.0001
    if (t < t0 || t > tEnd) return 0
    if (t <= tAtt) return expRamp(t, t0, floor, tAtt, layer.gain)
    if (t <= tMid) return expRamp(t, tAtt, layer.gain, tMid, layer.gain * 0.65)
    return expRamp(t, tMid, layer.gain * 0.65, tEnd, floor)
  }

  const osc = (wave: 'sine' | 'triangle', phase: number) => {
    const p = ((phase % 1) + 1) % 1
    if (wave === 'sine') return Math.sin(2 * Math.PI * p)
    return p < 0.5 ? 4 * p - 1 : 3 - 4 * p
  }

  for (const layer of layers) {
    let phase = 0
    const phaseInc = layer.freq / sampleRate
    const i0 = Math.floor(layer.start * sampleRate)
    const i1 = Math.min(n, Math.floor((layer.start + layer.dur + 0.03) * sampleRate))
    for (let i = i0; i < i1; i++) {
      const t = i / sampleRate
      const g = layerGainAt(layer, t)
      if (g > 0) mixed[i]! += osc(layer.wave, phase) * g
      phase += phaseInc
    }
  }

  // RBJ biquad low-pass (1400Hz, Q=0.7) — same as original BiquadFilterNode.
  const w0 = (2 * Math.PI * 1400) / sampleRate
  const alpha = Math.sin(w0) / (2 * 0.7)
  const cosW0 = Math.cos(w0)
  const b0 = (1 - cosW0) / 2
  const b1 = 1 - cosW0
  const b2 = (1 - cosW0) / 2
  const a0 = 1 + alpha
  const a1 = -2 * cosW0
  const a2 = 1 - alpha
  const b0n = b0 / a0
  const b1n = b1 / a0
  const b2n = b2 / a0
  const a1n = a1 / a0
  const a2n = a2 / a0
  const filtered = new Float32Array(n)
  let x1 = 0
  let x2 = 0
  let y1 = 0
  let y2 = 0
  for (let i = 0; i < n; i++) {
    const x0 = mixed[i]!
    const y0 = b0n * x0 + b1n * x1 + b2n * x2 - a1n * y1 - a2n * y2
    filtered[i] = y0
    x2 = x1
    x1 = x0
    y2 = y1
    y1 = y0
  }

  const pcm = new Int16Array(n)
  for (let i = 0; i < n; i++) {
    const v = Math.max(-1, Math.min(1, filtered[i]! * masterGain))
    pcm[i] = Math.floor(v * 32767)
  }

  const dataBytes = pcm.length * 2
  const buffer = new ArrayBuffer(44 + dataBytes)
  const view = new DataView(buffer)
  const writeStr = (offset: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(offset + i, s.charCodeAt(i))
  }
  writeStr(0, 'RIFF')
  view.setUint32(4, 36 + dataBytes, true)
  writeStr(8, 'WAVE')
  writeStr(12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true)
  view.setUint16(22, 1, true)
  view.setUint32(24, sampleRate, true)
  view.setUint32(28, sampleRate * 2, true)
  view.setUint16(32, 2, true)
  view.setUint16(34, 16, true)
  writeStr(36, 'data')
  view.setUint32(40, dataBytes, true)
  new Int16Array(buffer, 44).set(pcm)
  return new Blob([buffer], { type: 'audio/wav' })
}

function getWebChimeAudio(): HTMLAudioElement {
  if (webAudioEl) return webAudioEl
  const blob = synthesizeChimeWav()
  webObjectUrl = URL.createObjectURL(blob)
  webAudioEl = new Audio(webObjectUrl)
  webAudioEl.preload = 'auto'
  return webAudioEl
}

function stopWebKeepWarm(): void {
  if (webKeepWarmTimer == null) return
  clearInterval(webKeepWarmTimer)
  webKeepWarmTimer = null
}

function startWebKeepWarm(): void {
  if (typeof window === 'undefined' || webKeepWarmTimer != null) return
  webKeepWarmTimer = setInterval(() => {
    void unlockWebAudio().catch(() => {})
  }, WEB_KEEP_WARM_MS)
}

async function unlockWebAudio(): Promise<void> {
  const audio = getWebChimeAudio()
  const prevVol = audio.volume
  try {
    audio.muted = true
    audio.volume = 0
    await audio.play()
    audio.pause()
    audio.currentTime = 0
  } finally {
    audio.muted = false
    audio.volume = prevVol || 1
  }
}

async function playWebHtmlAudio(): Promise<void> {
  const audio = getWebChimeAudio()
  audio.pause()
  audio.currentTime = 0
  audio.muted = false
  audio.volume = 1
  await audio.play()
  console.info('[sound] played task-complete chime (HTMLAudio)')
}

/**
 * Unlock web playback during a user gesture. Desktop (Tauri) uses native OS
 * audio and does not need WebView priming.
 */
export function primeTaskCompleteAudio(): void {
  if (isTauriRuntime()) return
  startWebKeepWarm()
  void unlockWebAudio()
    .then(() => console.info('[sound] HTMLAudio primed'))
    .catch(err => console.warn('[sound] HTMLAudio prime failed', err))
}

/** Stop web keep-warm when the turn is cancelled without a Done chime. */
export function disarmTaskCompleteAudio(): void {
  stopWebKeepWarm()
}

export async function playTaskCompleteSound(): Promise<void> {
  const nowMs = Date.now()
  if (nowMs - lastPlayAtMs < PLAY_DEBOUNCE_MS) {
    console.info('[sound] skip task-complete chime (debounced)')
    return
  }
  lastPlayAtMs = nowMs
  stopWebKeepWarm()

  if (isTauriRuntime()) {
    try {
      await invoke('play_task_complete_chime')
      console.info('[sound] played task-complete chime (native)')
      return
    } catch (err) {
      console.warn('[sound] native chime failed; falling back to HTMLAudio', err)
    }
  }

  try {
    await playWebHtmlAudio()
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
    stopWebKeepWarm()
    return
  }
  void playTaskCompleteSound()
}
