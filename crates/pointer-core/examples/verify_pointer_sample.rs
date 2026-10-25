//! Render a verify-style before frame with the pointer at a chosen point.
//!
//! ```sh
//! cargo run -p pointer-core --example verify_pointer_sample -- \
//!     <raw.jpg> <out.jpg> <x> <y> [logical_w logical_h]
//! ```
//!
//! The production capture resamples the physical bitmap down to logical points
//! (`resample_capture_to_logical`), which is why `local_on_monitor` is a plain
//! subtraction. A `screencapture` JPEG is still physical, so pass the logical
//! size explicitly or the pointer lands at half the offset.

use pointer_core::agents::computer::vision::screen_overlay::redraw_with_current_pointer;
use pointer_core::agents::computer::vision::screen::MonitorInfo;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!(
            "usage: {} <raw.jpg> <out.jpg> <x> <y> [logical_w logical_h]",
            args[0]
        );
        std::process::exit(2);
    }
    let raw_path = &args[1];
    let out_path = &args[2];
    let x: i32 = args[3].parse().expect("x");
    let y: i32 = args[4].parse().expect("y");

    let raw = std::fs::read(raw_path).expect("read raw jpeg");

    let (logical_w, logical_h) = if args.len() >= 7 {
        (
            args[5].parse::<u32>().expect("logical_w"),
            args[6].parse::<u32>().expect("logical_h"),
        )
    } else {
        let img = image::load_from_memory(&raw).expect("decode").to_rgba8();
        (img.width(), img.height())
    };

    // Resample to logical size when the source is physical, mirroring
    // `resample_capture_to_logical`.
    let src = image::load_from_memory(&raw).expect("decode").to_rgba8();
    let (sw, sh) = (src.width(), src.height());
    let resampled = if (sw, sh) != (logical_w, logical_h) {
        eprintln!("resampling {sw}x{sh} -> {logical_w}x{logical_h}");
        image::imageops::resize(
            &src,
            logical_w,
            logical_h,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        src
    };
    let mut buf = Vec::new();
    resampled
        .write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Png,
        )
        .expect("encode png");

    let monitor = MonitorInfo::new(0, 0, logical_w as i32, logical_h as i32);
    let out = redraw_with_current_pointer(&buf, &monitor, (x, y)).expect("redraw");
    std::fs::write(out_path, &out).expect("write out");
    println!("wrote {out_path} ({} bytes), pointer at ({x}, {y})", out.len());
}
