//! Desktop task-complete chime — bypasses WKWebView / WebView2 Web Audio silence.
//!
//! Timbre matches the original Web Audio graph in `taskCompleteSound.ts` (historical):
//! four layers (G3/G4 sine+triangle, then D4/D5), low-pass 1400Hz, master 1.45.
//!
//! Playback via OS audio stack:
//! - macOS: `afplay`
//! - Windows: `System.Media.SoundPlayer`
//! - Linux: `paplay` / `aplay` / `ffplay` (first available)

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Match typical AudioContext sample rate used by the original Web Audio path.
const SAMPLE_RATE: u32 = 44_100;
const MASTER_GAIN: f32 = 1.45;
const LOWPASS_HZ: f32 = 1400.0;
const LOWPASS_Q: f32 = 0.7;
/// Filename bump when synthesis changes so a stale temp WAV is not reused.
const WAV_FILE_NAME: &str = "task-complete-chime-v3.wav";

#[derive(Clone, Copy)]
struct ToneLayer {
    freq: f32,
    /// 0 = sine, 1 = triangle
    wave: u8,
    gain: f32,
    start: f32,
    attack: f32,
    dur: f32,
}

/// Same layers as the original `playWithContext` in taskCompleteSound.ts.
const LAYERS: [ToneLayer; 4] = [
    ToneLayer {
        freq: 196.0,
        wave: 0,
        gain: 0.26,
        start: 0.0,
        attack: 0.016,
        dur: 0.3,
    },
    ToneLayer {
        freq: 392.0,
        wave: 1,
        gain: 0.2,
        start: 0.0,
        attack: 0.012,
        dur: 0.26,
    },
    ToneLayer {
        freq: 293.66,
        wave: 0,
        gain: 0.28,
        start: 0.11,
        attack: 0.018,
        dur: 0.38,
    },
    ToneLayer {
        freq: 587.33,
        wave: 1,
        gain: 0.14,
        start: 0.11,
        attack: 0.014,
        dur: 0.32,
    },
];

fn osc_sample(wave: u8, phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    if wave == 0 {
        (2.0 * std::f32::consts::PI * p).sin()
    } else {
        // Triangle in [-1, 1]
        if p < 0.5 {
            4.0 * p - 1.0
        } else {
            3.0 - 4.0 * p
        }
    }
}

/// Mirror Web Audio `exponentialRampToValueAtTime` between two points.
fn exp_ramp(t: f32, t0: f32, v0: f32, t1: f32, v1: f32) -> f32 {
    if t <= t0 {
        return v0;
    }
    if t >= t1 {
        return v1;
    }
    let ratio = (t - t0) / (t1 - t0);
    v0 * (v1 / v0).powf(ratio)
}

fn layer_gain_at(layer: &ToneLayer, t: f32) -> f32 {
    let t0 = layer.start;
    let t_att = t0 + layer.attack;
    let t_mid = t_att + 0.07;
    let t_end = t0 + layer.dur;
    let peak = layer.gain;
    let mid = layer.gain * 0.65;
    const FLOOR: f32 = 0.0001;
    if t < t0 || t > t_end {
        return 0.0;
    }
    if t <= t_att {
        return exp_ramp(t, t0, FLOOR, t_att, peak);
    }
    if t <= t_mid {
        return exp_ramp(t, t_att, peak, t_mid, mid);
    }
    exp_ramp(t, t_mid, mid, t_end, FLOOR)
}

fn apply_lowpass(input: &[f32]) -> Vec<f32> {
    // RBJ biquad low-pass, same target as Web Audio BiquadFilterNode.
    let sr = SAMPLE_RATE as f32;
    let w0 = 2.0 * std::f32::consts::PI * LOWPASS_HZ / sr;
    let alpha = w0.sin() / (2.0 * LOWPASS_Q);
    let cos_w0 = w0.cos();
    let b0 = (1.0 - cos_w0) / 2.0;
    let b1 = 1.0 - cos_w0;
    let b2 = (1.0 - cos_w0) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;
    let b0n = b0 / a0;
    let b1n = b1 / a0;
    let b2n = b2 / a0;
    let a1n = a1 / a0;
    let a2n = a2 / a0;

    let mut out = vec![0.0f32; input.len()];
    let mut x1 = 0.0f32;
    let mut x2 = 0.0f32;
    let mut y1 = 0.0f32;
    let mut y2 = 0.0f32;
    for (i, &x0) in input.iter().enumerate() {
        let y0 = b0n * x0 + b1n * x1 + b2n * x2 - a1n * y1 - a2n * y2;
        out[i] = y0;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
    }
    out
}

fn synthesize_chime_pcm() -> Vec<i16> {
    let total_secs = 0.55_f32;
    let n = (SAMPLE_RATE as f32 * total_secs) as usize;
    let mut mixed = vec![0.0f32; n];

    for layer in &LAYERS {
        let mut phase = 0.0f32;
        let phase_inc = layer.freq / SAMPLE_RATE as f32;
        let i0 = (layer.start * SAMPLE_RATE as f32) as usize;
        let i1 = ((layer.start + layer.dur + 0.03) * SAMPLE_RATE as f32) as usize;
        for i in i0..i1.min(n) {
            let t = i as f32 / SAMPLE_RATE as f32;
            let g = layer_gain_at(layer, t);
            if g <= 0.0 {
                phase += phase_inc;
                continue;
            }
            mixed[i] += osc_sample(layer.wave, phase) * g;
            phase += phase_inc;
        }
    }

    let filtered = apply_lowpass(&mixed);
    let mut pcm = vec![0i16; n];
    for (i, &s) in filtered.iter().enumerate() {
        let v = (s * MASTER_GAIN).clamp(-1.0, 1.0);
        pcm[i] = (v * 32767.0) as i16;
    }
    pcm
}

fn pcm_to_wav(pcm: &[i16]) -> Vec<u8> {
    let data_bytes = pcm.len() * 2;
    let mut out = Vec::with_capacity(44 + data_bytes);
    let channels: u16 = 1;
    let bits: u16 = 16;
    let byte_rate = SAMPLE_RATE * u32::from(channels) * u32::from(bits) / 8;
    let block_align = channels * bits / 8;

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn ensure_chime_wav_path() -> Result<PathBuf, String> {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    if let Some(p) = PATH.get() {
        if p.is_file() {
            return Ok(p.clone());
        }
    }
    let dir = std::env::temp_dir().join("pointer-app");
    fs::create_dir_all(&dir).map_err(|e| format!("create temp dir: {e}"))?;
    let path = dir.join(WAV_FILE_NAME);
    let wav = pcm_to_wav(&synthesize_chime_pcm());
    let mut f = fs::File::create(&path).map_err(|e| format!("create wav: {e}"))?;
    f.write_all(&wav).map_err(|e| format!("write wav: {e}"))?;
    let _ = PATH.set(path.clone());
    Ok(path)
}

fn spawn_player(program: &str, args: &[&str]) -> Result<(), String> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    Ok(())
}

fn play_wav_file(path: &PathBuf) -> Result<(), String> {
    let path_str = path.to_string_lossy().to_string();

    #[cfg(target_os = "macos")]
    {
        return spawn_player("afplay", &[&path_str]);
    }

    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "(New-Object System.Media.SoundPlayer -ArgumentList '{}').PlaySync()",
            path_str.replace('\'', "''")
        );
        return spawn_player(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        );
    }

    #[cfg(target_os = "linux")]
    {
        for (bin, extra) in [
            ("paplay", None),
            ("aplay", Some("-q")),
            ("ffplay", Some("-nodisp")),
        ] {
            let mut args: Vec<&str> = Vec::new();
            if let Some(flag) = extra {
                args.push(flag);
            }
            if bin == "ffplay" {
                args.push("-autoexit");
                args.push("-loglevel");
                args.push("quiet");
            }
            args.push(&path_str);
            if spawn_player(bin, &args).is_ok() {
                return Ok(());
            }
        }
        return Err("no paplay/aplay/ffplay available".into());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = path_str;
        Err("native task-complete chime unsupported on this OS".into())
    }
}

/// Play the task-complete chime through the OS audio stack (not the WebView).
#[tauri::command]
pub fn play_task_complete_chime() -> Result<(), String> {
    let path = ensure_chime_wav_path()?;
    play_wav_file(&path)?;
    log::info!(
        "task_complete_sound: queued native chime path={}",
        path.display()
    );
    Ok(())
}
