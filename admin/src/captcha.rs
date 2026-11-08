//! 登录验证码。
//!
//! 目的是挡住脚本化撞库：每次登录必须先拿一张图、看图填字，答错即作废。
//! 答案只存 5 分钟、单次有效；渲染成 SVG 直接回给前端当 `<img src>` 用，
//! 不引入图像编码库。

use crate::db::{now_ts, AdminStore};
use anyhow::Result;
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use sqlx::any::Any;
use sqlx::Row;

/// 有效期。够人看清再输入，又不至于让图长期可复用。
pub const CAPTCHA_TTL_SECS: i64 = 300;

/// 去掉 `0O1I` 这类易混字符——验证码是为了拦脚本，不该难住人。
const CHARSET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const CODE_LEN: usize = 4;

fn random_code() -> String {
    let mut rng = rand::thread_rng();
    (0..CODE_LEN)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

/// 答案不落明文：库里只存 `sha256(小写答案 + id)`，泄库也还原不出字符。
fn digest(id: &str, answer: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(answer.to_lowercase().as_bytes());
    hasher.update(id.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 5×7 点阵字体。只收验证码用得到的字符，避免引一套字体渲染依赖。
///
/// 用点阵而不是 `<text>`：SVG 里的字符是可被脚本直接读出来的，
/// 那样等于把答案印在图上。位图至少得先做 OCR。
const GLYPHS: &[(char, [u8; 7])] = &[
    ('2', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111]),
    ('3', [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110]),
    ('4', [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
    ('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
    ('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]),
    ('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]),
    ('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
    ('9', [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100]),
    ('A', [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('B', [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]),
    ('C', [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110]),
    ('D', [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110]),
    ('E', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111]),
    ('F', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('G', [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111]),
    ('H', [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('J', [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100]),
    ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
    ('L', [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
    ('M', [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001]),
    ('N', [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001]),
    ('P', [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('Q', [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101]),
    ('R', [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001]),
    ('S', [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110]),
    ('T', [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('U', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('V', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
    ('W', [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001]),
    ('X', [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
    ('Y', [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('Z', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
];

fn glyph(ch: char) -> Option<[u8; 7]> {
    GLYPHS.iter().find(|(c, _)| *c == ch).map(|(_, g)| *g)
}

const IMG_W: u32 = 120;
const IMG_H: u32 = 40;
const SCALE: i32 = 4;

/// 画一张 120×40 的 RGB 位图：点阵字符 + 几条干扰线。
fn render_png(code: &str) -> Result<Vec<u8>> {
    let mut rng = rand::thread_rng();
    // 背景 #F2F2F4
    let mut px = vec![0xF2u8; (IMG_W * IMG_H * 3) as usize];
    for i in (0..px.len()).step_by(3) {
        px[i] = 0xF2;
        px[i + 1] = 0xF2;
        px[i + 2] = 0xF4;
    }

    let mut set = |x: i32, y: i32, rgb: [u8; 3]| {
        if x < 0 || y < 0 || x >= IMG_W as i32 || y >= IMG_H as i32 {
            return;
        }
        let idx = ((y as u32 * IMG_W + x as u32) * 3) as usize;
        px[idx] = rgb[0];
        px[idx + 1] = rgb[1];
        px[idx + 2] = rgb[2];
    };

    // 干扰线：#D4D4D8
    for _ in 0..4 {
        let (x1, y1) = (rng.gen_range(0..IMG_W as i32), rng.gen_range(0..IMG_H as i32));
        let (x2, y2) = (rng.gen_range(0..IMG_W as i32), rng.gen_range(0..IMG_H as i32));
        let steps = (x2 - x1).abs().max((y2 - y1).abs()).max(1);
        for s in 0..=steps {
            let x = x1 + (x2 - x1) * s / steps;
            let y = y1 + (y2 - y1) * s / steps;
            set(x, y, [0xD4, 0xD4, 0xD8]);
        }
    }

    // 字符：#18181B，每个字带轻微抖动
    for (i, ch) in code.chars().enumerate() {
        let Some(bits) = glyph(ch) else { continue };
        let ox = 14 + i as i32 * 26 + rng.gen_range(-1..2);
        let oy = 6 + rng.gen_range(-2..3);
        for (row, line) in bits.iter().enumerate() {
            for col in 0..5 {
                if line & (1 << (4 - col)) == 0 {
                    continue;
                }
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        set(
                            ox + col as i32 * SCALE + dx,
                            oy + row as i32 * SCALE + dy,
                            [0x18, 0x18, 0x1B],
                        );
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, IMG_W, IMG_H);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&px)?;
    }
    Ok(out)
}

/// 发一张新验证码：写库 + 返回 id 与可直接嵌进 `<img>` 的 data URI。
pub async fn issue(store: &AdminStore) -> Result<(String, String)> {
    let code = random_code();
    let id = format!("{:032x}", rand::thread_rng().gen::<u128>());
    let now = now_ts();
    let sql = store.sql(
        "INSERT INTO captchas (id, answer_hash, expires_at, used_at, created_at) \
         VALUES (?, ?, ?, NULL, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(&id)
        .bind(digest(&id, &code))
        .bind(now + CAPTCHA_TTL_SECS)
        .bind(now)
        .execute(store.pool())
        .await?;

    let png_bytes = render_png(&code)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    Ok((id, format!("data:image/png;base64,{b64}")))
}

/// 校验并作废。无论对错都标记已用——答错必须换一张图。
pub async fn consume(store: &AdminStore, id: &str, answer: &str) -> Result<bool> {
    let sql = store.sql("SELECT answer_hash, expires_at, used_at FROM captchas WHERE id = ?");
    let Some(row) = sqlx::query::<Any>(&sql)
        .bind(id)
        .fetch_optional(store.pool())
        .await?
    else {
        return Ok(false);
    };
    let stored: String = row.try_get("answer_hash")?;
    let expires_at: i64 = row.try_get("expires_at")?;
    let used_at: Option<i64> = row.try_get("used_at")?;

    if used_at.is_some() || expires_at < now_ts() {
        return Ok(false);
    }

    let mark = store.sql("UPDATE captchas SET used_at = ? WHERE id = ? AND used_at IS NULL");
    let changed = sqlx::query::<Any>(&mark)
        .bind(now_ts())
        .bind(id)
        .execute(store.pool())
        .await?
        .rows_affected();
    if changed == 0 {
        // 并发下被别的请求先消费了。
        return Ok(false);
    }

    Ok(digest(id, answer) == stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_uses_unambiguous_charset() {
        for _ in 0..50 {
            let code = random_code();
            assert_eq!(code.len(), CODE_LEN);
            for ch in code.chars() {
                assert!(CHARSET.contains(&(ch as u8)), "出现了易混字符: {ch}");
            }
        }
    }

    #[test]
    fn digest_is_case_insensitive_and_id_bound() {
        assert_eq!(digest("id1", "AbCd"), digest("id1", "abcd"));
        assert_ne!(digest("id1", "abcd"), digest("id2", "abcd"));
    }

    #[test]
    fn renders_png_not_plaintext() {
        let png = render_png("AB2C").expect("png");
        // PNG magic number：确认真的编成了位图，而不是把字符写在文本里。
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        // 答案不能以明文出现在输出里。
        assert!(!png.windows(4).any(|w| w == b"AB2C"));
    }

    #[test]
    fn every_charset_char_has_a_glyph() {
        for &b in CHARSET {
            let ch = b as char;
            assert!(glyph(ch).is_some(), "缺少字形: {ch}");
        }
    }
}
