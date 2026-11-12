// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The media worker as a process (ADR-0048 rule 2): `weavesignal-control media-worker normalise`
//! for audio, `media-worker still` for a picture (the video slate's, ADR-0010) and
//! `media-worker brand` for the station's brand icon and banner (ADR-0237).
//!
//! It takes an untrusted file on standard input and writes clean FLAC (or a clean PNG) to
//! standard output, and one JSON line to standard error: the report, or `{"error": ...}` with
//! exit status 3.
//! Before it reads a byte it restricts itself: no file system, no network, no signals to
//! other processes (Landlock, as far as the kernel offers it), no new processes or threads,
//! and bounded CPU time and memory (resource limits). The caller kills it after a timeout too.

use std::io::{Read, Write};
use std::process::ExitCode;

use crate::{DEFAULT_MAX_SECONDS, Target, process};

/// The largest upload read, in bytes: ten minutes of 24-bit stereo WAV at 96 kHz is 346 MB, but
/// a cart that size is a mistake; compressed files are far smaller.
const MAX_INPUT: u64 = 256 * 1024 * 1024;

/// The largest picture read: a photograph is a few megabytes.
const MAX_STILL_INPUT: u64 = 24 * 1024 * 1024;

fn fail(message: &str) -> ExitCode {
    eprintln!("{}", serde_json::json!({ "error": message }));
    ExitCode::from(3)
}

/// The sandbox's outcome, reported with the result so a weaker one is visible.
fn sandbox() -> Result<&'static str, String> {
    use rustix::process::{Resource, Rlimit, setrlimit};
    let limit = |r, n: u64| {
        setrlimit(
            r,
            Rlimit {
                current: Some(n),
                maximum: Some(n),
            },
        )
        .map_err(|e| format!("cannot set a resource limit: {e}"))
    };
    limit(Resource::Cpu, 120)?;
    limit(Resource::As, 2 * 1024 * 1024 * 1024)?;
    // Standard output may be a file the caller opened; Landlock keeps the worker from opening any.
    limit(Resource::Fsize, 1024 * 1024 * 1024)?;
    limit(Resource::Nproc, 0)?;
    limit(Resource::Core, 0)?;

    use landlock::{ABI, Access, AccessFs, AccessNet, Ruleset, RulesetAttr, RulesetStatus, Scope};
    let abi = ABI::V6;
    let status = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .and_then(|r| r.handle_access(AccessNet::from_all(abi)))
        .and_then(|r| r.scope(Scope::from_all(abi)))
        .and_then(|r| r.create())
        .and_then(|r| r.restrict_self())
        .map_err(|e| format!("cannot restrict the worker: {e}"))?;
    Ok(match status.ruleset {
        RulesetStatus::FullyEnforced => "landlock",
        RulesetStatus::PartiallyEnforced => "landlock-partial",
        RulesetStatus::NotEnforced => "rlimits-only",
    })
}

fn arg(args: &[String], name: &str, default: f64) -> Result<f64, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(default),
        Some(i) => args
            .get(i + 1)
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("{name} takes a number")),
    }
}

/// The worker's whole life, given its arguments (after the program name and any subcommand
/// that led here): `normalise [--target-lufs N] [--ceiling-dbtp N] [--max-seconds N]`.
pub fn run(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("normalise") => normalise(args),
        Some("still") => still(args),
        Some("avatar") => avatar(args),
        Some("picture") => picture(args),
        Some("brand") => brand(args),
        _ => {
            eprintln!(
                "usage: media-worker normalise [--target-lufs -23] [--ceiling-dbtp -1] [--max-seconds 600] < in > out.flac\n       media-worker still [--max-width 854] [--max-height 480] < in > out.png\n       media-worker avatar [--side 128] < in > out.png\n       media-worker picture [--long-side 1600] [--thumb-side 480] < in > picture-then-thumbnail\n       media-worker brand icon|banner < in > out.png"
            );
            ExitCode::from(2)
        }
    }
}

/// `brand icon|banner`: a PNG or JPEG in, the station's brand picture out as a clean PNG
/// (ADR-0237).
fn brand(args: &[String]) -> ExitCode {
    let Some(kind) = args.get(1).and_then(|k| crate::brand::Kind::parse(k)) else {
        return fail("brand takes icon or banner");
    };
    let sandboxed = match sandbox() {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let max = kind.max_bytes() as u64;
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin().take(max + 1).read_to_end(&mut input) {
        return fail(&format!("cannot read the file: {e}"));
    }
    match crate::brand::process_brand(&input, kind) {
        Ok((png, report)) => {
            let mut out = std::io::stdout().lock();
            if let Err(e) = out.write_all(&png).and_then(|()| out.flush()) {
                return fail(&format!("cannot write the picture: {e}"));
            }
            let mut v = serde_json::to_value(&report).expect("the report serialises");
            v["sandbox"] = sandboxed.into();
            eprintln!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

/// `still [--max-width N] [--max-height N]`: a PNG or JPEG in, a PNG that fits out.
fn still(args: &[String]) -> ExitCode {
    let size = |name: &str, default: f64| -> Result<usize, String> {
        let v = arg(args, name, default)?;
        if (16.0..=8192.0).contains(&v) && v.fract() == 0.0 {
            Ok(v as usize)
        } else {
            Err(format!("{name} is a whole number of pixels, 16 to 8192"))
        }
    };
    let (max_w, max_h) = match (size("--max-width", 854.0), size("--max-height", 480.0)) {
        (Ok(w), Ok(h)) => (w, h),
        (Err(e), _) | (_, Err(e)) => return fail(&e),
    };
    let sandboxed = match sandbox() {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin()
        .take(MAX_STILL_INPUT + 1)
        .read_to_end(&mut input)
    {
        return fail(&format!("cannot read the file: {e}"));
    }
    if input.len() as u64 > MAX_STILL_INPUT {
        return fail(&format!(
            "the picture is larger than {} MB",
            MAX_STILL_INPUT / 1024 / 1024
        ));
    }
    match crate::still::process_still(&input, max_w, max_h) {
        Ok((png, report)) => {
            let mut out = std::io::stdout().lock();
            if let Err(e) = out.write_all(&png).and_then(|()| out.flush()) {
                return fail(&format!("cannot write the picture: {e}"));
            }
            let mut v = serde_json::to_value(&report).expect("the report serialises");
            v["sandbox"] = sandboxed.into();
            eprintln!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

/// The largest avatar upload read: the browser sends a cropped square of a few hundred
/// kilobytes; the caller's own limit is lower still.
const MAX_AVATAR_INPUT: u64 = 8 * 1024 * 1024;

/// `avatar [--side N]`: a PNG or JPEG in, its middle square as a small clean PNG out (the chat's
/// `weavesignal-chat media-worker avatar`, ADR-0235).
fn avatar(args: &[String]) -> ExitCode {
    let side = match arg(args, "--side", crate::avatar::DEFAULT_SIDE as f64) {
        Ok(v) if (32.0..=512.0).contains(&v) && v.fract() == 0.0 => v as usize,
        Ok(_) => return fail("--side is a whole number of pixels, 32 to 512"),
        Err(e) => return fail(&e),
    };
    let sandboxed = match sandbox() {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin()
        .take(MAX_AVATAR_INPUT + 1)
        .read_to_end(&mut input)
    {
        return fail(&format!("cannot read the file: {e}"));
    }
    if input.len() as u64 > MAX_AVATAR_INPUT {
        return fail(&format!(
            "the picture is larger than {} MB",
            MAX_AVATAR_INPUT / 1024 / 1024
        ));
    }
    match crate::avatar::process_avatar(&input, side) {
        Ok((png, report)) => {
            let mut out = std::io::stdout().lock();
            if let Err(e) = out.write_all(&png).and_then(|()| out.flush()) {
                return fail(&format!("cannot write the picture: {e}"));
            }
            let mut v = serde_json::to_value(&report).expect("the report serialises");
            v["sandbox"] = sandboxed.into();
            eprintln!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

/// The largest chat picture read: the caller's own limit (`attachments.max_picture_kb`) is lower.
const MAX_PICTURE_INPUT: u64 = 32 * 1024 * 1024;

/// `picture [--long-side N] [--thumb-side N]`: a PNG, JPEG or WebP in; out, the picture as the
/// chat shows it (a PNG, or a JPEG for a photograph) followed by its thumbnail, with the report
/// saying where the first ends (ADR-0262).
fn picture(args: &[String]) -> ExitCode {
    let size = |name: &str, default: usize, lo: f64, hi: f64| -> Result<usize, String> {
        let v = arg(args, name, default as f64)?;
        if (lo..=hi).contains(&v) && v.fract() == 0.0 {
            Ok(v as usize)
        } else {
            Err(format!("{name} is a whole number of pixels, {lo} to {hi}"))
        }
    };
    let (long, thumb) = match (
        size(
            "--long-side",
            crate::picture::DEFAULT_LONG_SIDE,
            640.0,
            4096.0,
        ),
        size(
            "--thumb-side",
            crate::picture::DEFAULT_THUMB_SIDE,
            64.0,
            1024.0,
        ),
    ) {
        (Ok(l), Ok(t)) => (l, t),
        (Err(e), _) | (_, Err(e)) => return fail(&e),
    };
    let sandboxed = match sandbox() {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin()
        .take(MAX_PICTURE_INPUT + 1)
        .read_to_end(&mut input)
    {
        return fail(&format!("cannot read the file: {e}"));
    }
    if input.len() as u64 > MAX_PICTURE_INPUT {
        return fail(&format!(
            "the picture is larger than {} MB",
            MAX_PICTURE_INPUT / 1024 / 1024
        ));
    }
    match crate::picture::process_picture(&input, long, thumb) {
        Ok((full, small, report)) => {
            let mut out = std::io::stdout().lock();
            if let Err(e) = out
                .write_all(&full)
                .and_then(|()| out.write_all(&small))
                .and_then(|()| out.flush())
            {
                return fail(&format!("cannot write the picture: {e}"));
            }
            let mut v = serde_json::to_value(&report).expect("the report serialises");
            v["sandbox"] = sandboxed.into();
            eprintln!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

/// `normalise [--target-lufs N] [--ceiling-dbtp N] [--max-seconds N]`: audio in, FLAC out.
fn normalise(args: &[String]) -> ExitCode {
    let parsed = (|| -> Result<(Target, f64), String> {
        let target = Target {
            lufs: arg(args, "--target-lufs", Target::default().lufs)?,
            ceiling_dbtp: arg(args, "--ceiling-dbtp", Target::default().ceiling_dbtp)?,
        };
        if !(-40.0..=-5.0).contains(&target.lufs) || !(-10.0..=0.0).contains(&target.ceiling_dbtp) {
            return Err("the target is -40 to -5 LUFS and the ceiling -10 to 0 dBTP".into());
        }
        let max = arg(args, "--max-seconds", DEFAULT_MAX_SECONDS)?;
        if !(1.0..=3600.0).contains(&max) {
            return Err("--max-seconds is 1 to 3600".into());
        }
        Ok((target, max))
    })();
    let (target, max_seconds) = match parsed {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let sandboxed = match sandbox() {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };

    let mut input = Vec::new();
    if let Err(e) = std::io::stdin().take(MAX_INPUT + 1).read_to_end(&mut input) {
        return fail(&format!("cannot read the file: {e}"));
    }
    if input.len() as u64 > MAX_INPUT {
        return fail(&format!(
            "the file is larger than {} MB",
            MAX_INPUT / 1024 / 1024
        ));
    }
    match process(&input, target, max_seconds) {
        Ok((flac, report)) => {
            let mut out = std::io::stdout().lock();
            if let Err(e) = out.write_all(&flac).and_then(|()| out.flush()) {
                return fail(&format!("cannot write the FLAC: {e}"));
            }
            let mut v = serde_json::to_value(&report).expect("the report serialises");
            v["sandbox"] = sandboxed.into();
            eprintln!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}
