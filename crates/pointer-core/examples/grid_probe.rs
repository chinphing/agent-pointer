//! Locate a grid in a screenshot by scanning for dark line pixels.
//!
//! ```sh
//! cargo run -p pointer-core --example grid_probe -- <image> [x0 y0 x1 y1] [threshold]
//! ```
//!
//! Prints the x of every vertical line and the y of every horizontal line inside
//! the search window, with the dark-pixel count for each. A sudoku board shows
//! ten of each: two heavy borders and eight light rules.

use image::GenericImageView;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: {} <image> [x0 y0 x1 y1] [threshold]",
            args[0]
        );
        std::process::exit(2);
    }
    let img = image::open(&args[1]).expect("open image").to_rgba8();
    let (w, h) = img.dimensions();

    let (x0, y0, x1, y1) = if args.len() >= 6 {
        (
            args[2].parse::<u32>().unwrap(),
            args[3].parse::<u32>().unwrap(),
            args[4].parse::<u32>().unwrap(),
            args[5].parse::<u32>().unwrap(),
        )
    } else {
        (0, 0, w, h)
    };
    let threshold: u8 = if args.len() >= 7 {
        args[6].parse::<u8>().unwrap()
    } else {
        110
    };

    let x1 = x1.min(w);
    let y1 = y1.min(h);
    let dark = |x: u32, y: u32| -> bool {
        let p = img.get_pixel(x, y);
        (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3 < threshold as u32
    };

    // A line spans most of the window; a glyph or icon spans a fraction of it.
    let span_y = (y1 - y0) as f64;
    let span_x = (x1 - x0) as f64;
    let min_v = (span_y * 0.55) as u32;
    let min_h = (span_x * 0.55) as u32;

    let mut vlines: Vec<(u32, u32)> = Vec::new();
    for x in x0..x1 {
        let count = (y0..y1).filter(|&y| dark(x, y)).count() as u32;
        if count >= min_v {
            vlines.push((x, count));
        }
    }
    let mut hlines: Vec<(u32, u32)> = Vec::new();
    for y in y0..y1 {
        let count = (x0..x1).filter(|&x| dark(x, y)).count() as u32;
        if count >= min_h {
            hlines.push((y, count));
        }
    }

    let group = |lines: &[(u32, u32)]| -> Vec<(u32, u32, u32)> {
        // Adjacent coordinates belong to one thick line; report its span.
        let mut out: Vec<(u32, u32, u32)> = Vec::new();
        for &(pos, count) in lines {
            match out.last_mut() {
                Some(last) if pos == last.1 + 1 => {
                    last.1 = pos;
                    last.2 = last.2.max(count);
                }
                _ => out.push((pos, pos, count)),
            }
        }
        out
    };

    println!("window x[{x0},{x1}) y[{y0},{y1}) threshold={threshold}");
    println!("vertical lines (x_start..x_end, dark px):");
    for (a, b, c) in group(&vlines) {
        println!("  {a}..{b}  ({c})");
    }
    println!("horizontal lines (y_start..y_end, dark px):");
    for (a, b, c) in group(&hlines) {
        println!("  {a}..{b}  ({c})");
    }
}
