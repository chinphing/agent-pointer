//! One-time SVG captcha challenges for the `local_password` provider.

use rand::Rng;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CAPTCHA_TTL: Duration = Duration::from_secs(5 * 60);
/// Hard cap on live challenges. A dropped challenge only costs a refresh, so
/// evicting the oldest one is safe (unlike dropping an in-flight login).
const CAPTCHA_MAX_ENTRIES: usize = 512;
const CAPTCHA_LEN: usize = 4;
/// Ambiguity-avoiding alphabet (no 0/O, 1/I/l).
const CAPTCHA_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

struct CaptchaEntry {
    answer: String,
    expires_at: Instant,
}

/// In-memory one-time captcha challenges (process-local).
#[derive(Default)]
pub struct CaptchaStore {
    inner: Mutex<HashMap<String, CaptchaEntry>>,
}

impl CaptchaStore {
    pub fn issue(&self) -> (String, String) {
        self.purge_expired();
        let answer = random_captcha_text();
        let id = random_id();
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.len() >= CAPTCHA_MAX_ENTRIES {
            if let Some(oldest) = guard
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(id, _)| id.clone())
            {
                guard.remove(&oldest);
            }
        }
        guard.insert(
            id.clone(),
            CaptchaEntry {
                answer: answer.clone(),
                expires_at: Instant::now() + CAPTCHA_TTL,
            },
        );
        log::info!("captcha: issued id={id}");
        if cfg!(debug_assertions) {
            // Local smoke tests: answer is only logged in debug builds.
            log::info!("captcha: answer (debug only) id={id} answer={answer}");
        }
        (id, answer)
    }

    /// Consume and verify. Returns false on missing/expired/mismatch (entry removed either way).
    pub fn verify_and_consume(&self, id: &str, attempt: &str) -> bool {
        let id = id.trim();
        if id.is_empty() {
            return false;
        }
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.remove(id) else {
            log::warn!("captcha: id not found id={id}");
            return false;
        };
        if Instant::now() > entry.expires_at {
            log::warn!("captcha: expired id={id}");
            return false;
        }
        let ok = entry.answer.eq_ignore_ascii_case(attempt.trim());
        if !ok {
            log::warn!("captcha: mismatch id={id}");
        }
        ok
    }

    fn purge_expired(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        guard.retain(|_, e| e.expires_at > now);
    }
}

fn random_captcha_text() -> String {
    let mut rng = rand::thread_rng();
    (0..CAPTCHA_LEN)
        .map(|_| {
            let idx = rng.gen_range(0..CAPTCHA_ALPHABET.len());
            CAPTCHA_ALPHABET[idx] as char
        })
        .collect()
}

fn random_id() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Characters the store may issue (exposed for the glyph-coverage test).
#[cfg(test)]
pub(crate) fn captcha_alphabet() -> &'static [u8] {
    CAPTCHA_ALPHABET
}

/// Render the captcha as vector strokes.
///
/// No `<text>`: the answer never appears in the SVG source, so scraping the
/// response (or reading the DOM) yields geometry only.
pub fn render_captcha_svg(text: &str) -> String {
    const WIDTH: i32 = 140;
    const HEIGHT: i32 = 48;
    const PAD: f32 = 12.0;
    let mut rng = rand::thread_rng();

    let mut noise = String::new();
    for _ in 0..6 {
        let x1: i32 = rng.gen_range(0..WIDTH);
        let y1: i32 = rng.gen_range(0..HEIGHT);
        let x2: i32 = rng.gen_range(0..WIDTH);
        let y2: i32 = rng.gen_range(0..HEIGHT);
        // `#` is literal in format!; do not write `##` (invalid CSS color → black box).
        noise.push_str(&format!(
            "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"#c5c9d1\" stroke-width=\"1\"/>"
        ));
    }

    let glyphs: Vec<char> = text.chars().collect();
    let count = glyphs.len().max(1) as f32;
    let slot = (WIDTH as f32 - 2.0 * PAD) / count;
    let scale = (slot * 0.62 / crate::captcha_glyphs::GLYPH_W).min(2.8);

    let mut strokes = String::new();
    for (i, ch) in glyphs.iter().enumerate() {
        let cx = PAD + slot * (i as f32 + 0.5);
        let cy = HEIGHT as f32 / 2.0 + rng.gen_range(-3.0..3.0);
        let rot = rng.gen_range(-16.0..16.0);
        let ox = cx - crate::captcha_glyphs::GLYPH_W * scale / 2.0;
        let oy = cy - crate::captcha_glyphs::GLYPH_H * scale / 2.0;
        let glyph = crate::captcha_glyphs::glyph_strokes(*ch).unwrap_or(&[]);
        if glyph.is_empty() {
            // Unknown character: a box keeps the challenge solvable-looking
            // without ever writing the character itself.
            strokes.push_str(&format!(
                "<rect x=\"{ox:.1}\" y=\"{oy:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"none\" stroke=\"#1f2937\" stroke-width=\"2\"/>",
                crate::captcha_glyphs::GLYPH_W * scale,
                crate::captcha_glyphs::GLYPH_H * scale
            ));
            continue;
        }
        for stroke in glyph {
            let mut d = String::new();
            for (k, (x, y)) in stroke.iter().enumerate() {
                let px = ox + x * scale;
                let py = oy + y * scale;
                d.push_str(&format!("{}{px:.1},{py:.1}", if k == 0 { "M" } else { " L" }));
            }
            strokes.push_str(&format!(
                "<path d=\"{d}\" fill=\"none\" stroke=\"#1f2937\" stroke-width=\"2.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" transform=\"rotate({rot:.1} {cx:.1} {cy:.1})\"/>"
            ));
        }
    }

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{HEIGHT}\" viewBox=\"0 0 {WIDTH} {HEIGHT}\" role=\"img\" aria-label=\"captcha\">\
<rect width=\"100%\" height=\"100%\" fill=\"#f3f4f6\"/>\
{noise}\
{strokes}\
</svg>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the vector renderer: the answer must not be text.
    #[test]
    fn svg_never_carries_the_answer_as_text() {
        let svg = render_captcha_svg("YHES");
        assert!(!svg.contains("<text"), "captcha must not emit <text>: {svg}");
        assert!(!svg.contains("font-family"), "captcha must not depend on fonts");
        assert!(
            !svg.contains("YHES"),
            "answer must not appear literally in the SVG: {svg}"
        );
        // Every answer character is drawn: one path per stroke, no box fallback.
        assert!(svg.contains("<path"), "expected vector strokes: {svg}");
        assert!(!svg.contains("<rect x="), "no glyph should fall back to a box");
    }

    #[test]
    fn svg_is_stable_in_shape_for_every_issued_char() {
        for byte in captcha_alphabet() {
            let text = format!("{}{}{}{}", *byte as char, '7', 'K', '3');
            let svg = render_captcha_svg(&text);
            assert!(!svg.contains("<text"));
            assert!(!svg.contains(&text), "answer leaked for {text}");
            assert!(svg.starts_with("<svg "), "not an svg: {svg}");
            assert!(svg.ends_with("</svg>"));
        }
    }

    #[test]
    fn captcha_verify_consumes_once() {
        let store = CaptchaStore::default();
        let (id, answer) = store.issue();
        assert!(store.verify_and_consume(&id, &answer));
        assert!(!store.verify_and_consume(&id, &answer));
    }

    #[test]
    fn captcha_mismatch_fails() {
        let store = CaptchaStore::default();
        let (id, _) = store.issue();
        assert!(!store.verify_and_consume(&id, "XXXX"));
    }

    #[test]
    fn captcha_svg_draws_strokes_with_valid_colors() {
        let svg = render_captcha_svg("AB12");
        assert!(svg.contains("<svg"));
        // Characters are geometry now — the old `>A<` assertion was the leak.
        assert!(svg.contains("stroke=\"#1f2937\""));
        assert!(!svg.contains("<text"));
        // Valid CSS hex colors (a mistaken `##rrggbb` paints the whole tile black).
        assert!(svg.contains("fill=\"#f3f4f6\""));
        assert!(svg.contains("stroke=\"#c5c9d1\""));
        assert!(!svg.contains("##"));
    }
}
