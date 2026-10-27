//! Turn collected `locate` traces into `truth/truth_incidents.json`.
//!
//! ```bash
//! # 1. collect on the machine where it went wrong
//! POINTER_LOCATE_DEBUG_DIR=/tmp/locate_debug RUST_LOG=info <run the app>
//!
//! # 2. label one or more runs: open `<run>_frame.jpg`, measure the right box,
//! #    write it into `<run>.json` as `truth_bbox` (pixels of that JPEG)
//!
//! # 3. convert
//! cargo run -p pointer-core --example locate_trace_to_truth -- /tmp/locate_debug
//!
//! # 4. run the incidents dataset like any other truth file
//! POINTER_LOCATE_TRUTH=truth_incidents.json \
//!   cargo test -p pointer-core --test locate_fixture frozen_desktop_grounding \
//!   -- --ignored --nocapture
//! ```
//!
//! Output, relative to `tests/locate_fixture/`:
//!
//! | path | what it is |
//! |---|---|
//! | `truth/truth_incidents.json` | one case per labelled run |
//! | `fixtures/incidents/<run>.jpg` | that run's `frame.jpg`, copied verbatim |
//!
//! # Why the copy
//!
//! The trace directory lives on the machine that failed and gets cleaned up. The
//! case has to keep working without it, so the frame is copied in and the case
//! points at the copy. Copying also freezes the image: re-running the app can
//! never change what an existing case is measured against.
//!
//! # Rewritten, not appended
//!
//! The trace directory is the source of truth: the dataset is rebuilt from every
//! labelled trace in it on each run, so a case can be fixed by editing its trace
//! and converting again. Fixtures already copied are left alone — this never
//! deletes anything.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use pointer_core::agents::computer::locate::trace::LocateTrace;
use pointer_core::agents::computer::locate::truth::{case_from_trace, TruthFile};

/// File name inside `truth/`. `POINTER_LOCATE_TRUTH` takes exactly this.
const DATASET: &str = "truth_incidents.json";

fn usage() {
    eprintln!(
        "usage: locate_trace_to_truth <trace_dir> [--out <tests/locate_fixture>]\n\
         \n\
         <trace_dir> is a POINTER_LOCATE_DEBUG_DIR collection: <run>.json plus the\n\
         JPEGs it references. Every trace with `truth_bbox` filled in becomes one\n\
         case; the rest are reported and skipped."
    );
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut traces: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                let dir = args.next().context("--out needs a directory")?;
                out = Some(PathBuf::from(dir));
            }
            "-h" | "--help" => {
                usage();
                return Ok(());
            }
            other if other.starts_with('-') => bail!("unknown flag {other}"),
            other => {
                ensure!(traces.is_none(), "only one <trace_dir> is accepted");
                traces = Some(PathBuf::from(other));
            }
        }
    }
    let Some(traces) = traces else {
        usage();
        bail!("missing <trace_dir>");
    };
    // Default resolves against the crate, not the shell's cwd: `cargo run` from
    // the workspace root and from `crates/pointer-core` have to agree.
    let out = out
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/locate_fixture"));
    run(&traces, &out)
}

fn run(trace_dir: &Path, out: &Path) -> Result<()> {
    let fixture_dir = out.join("fixtures/incidents");
    let truth_dir = out.join("truth");

    let mut files: Vec<PathBuf> = std::fs::read_dir(trace_dir)
        .with_context(|| format!("read {}", trace_dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    // Sorted so two conversions of the same directory produce the same file,
    // byte for byte — otherwise every diff is noise.
    files.sort();
    ensure!(
        !files.is_empty(),
        "{} holds no *.json traces",
        trace_dir.display()
    );

    std::fs::create_dir_all(&fixture_dir)
        .with_context(|| format!("create {}", fixture_dir.display()))?;
    std::fs::create_dir_all(&truth_dir)
        .with_context(|| format!("create {}", truth_dir.display()))?;

    let mut cases = Vec::new();
    let mut unlabelled: Vec<String> = Vec::new();
    for path in files {
        let raw =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let trace: LocateTrace = serde_json::from_str(&raw)
            .with_context(|| format!("parse {} as a locate trace", path.display()))?;
        if trace.truth_bbox.is_none() {
            unlabelled.push(trace.run_id.clone());
            continue;
        }
        ensure!(
            !trace.run_id.is_empty() && !trace.run_id.contains(['/', '\\']),
            "{}: run_id {:?} is not usable as a file name",
            path.display(),
            trace.run_id
        );

        // The JPEG next to the trace is the fixture. Its **real** size is what
        // every box in the case is measured against, so it is read from the file
        // rather than trusted from the trace.
        let frame = trace_dir.join(&trace.frame.path);
        let (jw, jh) = image::image_dimensions(&frame).with_context(|| {
            format!(
                "read {} (the trace references it; is the collection complete?)",
                frame.display()
            )
        })?;
        let name = format!("fixtures/incidents/{}.jpg", trace.run_id);
        std::fs::copy(&frame, out.join(&name))
            .with_context(|| format!("copy {} -> {}", frame.display(), name))?;
        cases.push(case_from_trace(&trace, &name, [jw, jh])?);
    }

    if cases.is_empty() {
        bail!(
            "no trace in {} has a truth_bbox; {} unlabelled run(s): {}",
            trace_dir.display(),
            unlabelled.len(),
            unlabelled.join(", ")
        );
    }

    let dataset = TruthFile {
        fixture: String::new(),
        size: None,
        cases,
    };
    let path = truth_dir.join(DATASET);
    let mut json = serde_json::to_string_pretty(&dataset)?;
    json.push('\n');
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;

    println!("{} case(s) -> {}", dataset.cases.len(), path.display());
    println!("fixtures -> {}", fixture_dir.display());
    if !unlabelled.is_empty() {
        println!(
            "skipped {} unlabelled run(s): {}",
            unlabelled.len(),
            unlabelled.join(", ")
        );
    }
    println!(
        "run it: POINTER_LOCATE_TRUTH={DATASET} cargo test -p pointer-core \
         --test locate_fixture frozen_desktop_grounding -- --ignored --nocapture"
    );
    Ok(())
}
