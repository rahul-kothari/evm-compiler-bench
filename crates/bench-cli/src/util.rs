use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
    time::Instant,
};

use crate::models::CommandStats;

pub struct MeasuredOutput {
    pub output: Output,
    pub stats: CommandStats,
}

pub fn run_measured(command: &mut Command, stdin: Option<&[u8]>) -> Result<MeasuredOutput> {
    let before = usage();
    let start = Instant::now();
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().context("spawning command")?;
    if let Some(input) = stdin {
        child
            .stdin
            .as_mut()
            .context("opening child stdin")?
            .write_all(input)?;
    }
    let output = child.wait_with_output()?;
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    let after = usage();
    Ok(MeasuredOutput {
        output,
        stats: CommandStats {
            wall_ms,
            cpu_ms: (after.cpu_ms - before.cpu_ms).max(0.0),
            peak_rss_kib: after.peak_rss_kib.max(before.peak_rss_kib),
        },
    })
}

pub fn require_success(measured: MeasuredOutput, label: &str) -> Result<MeasuredOutput> {
    if measured.output.status.success() {
        return Ok(measured);
    }
    bail!(
        "{label} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        measured.output.status,
        String::from_utf8_lossy(&measured.output.stdout),
        String::from_utf8_lossy(&measured.output.stderr)
    );
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn strip_0x(value: &str) -> &str {
    value.strip_prefix("0x").unwrap_or(value)
}

pub fn byte_len(hex_value: &str) -> Result<usize> {
    Ok(hex::decode(strip_0x(hex_value))?.len())
}

pub fn stripped_cbor_len(hex_value: &str) -> Result<usize> {
    let bytes = hex::decode(strip_0x(hex_value))?;
    if bytes.len() < 2 {
        return Ok(bytes.len());
    }
    let metadata_len =
        u16::from_be_bytes([bytes[bytes.len() - 2], bytes[bytes.len() - 1]]) as usize;
    if metadata_len + 2 <= bytes.len() {
        Ok(bytes.len() - metadata_len - 2)
    } else {
        Ok(bytes.len())
    }
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))
}

#[cfg(unix)]
#[derive(Clone, Copy)]
struct Usage {
    cpu_ms: f64,
    peak_rss_kib: u64,
}

#[cfg(unix)]
fn usage() -> Usage {
    use libc::{RUSAGE_CHILDREN, getrusage, rusage};
    let mut raw = std::mem::MaybeUninit::<rusage>::zeroed();
    let ok = unsafe { getrusage(RUSAGE_CHILDREN, raw.as_mut_ptr()) };
    if ok != 0 {
        return Usage {
            cpu_ms: 0.0,
            peak_rss_kib: 0,
        };
    }
    let raw = unsafe { raw.assume_init() };
    let user_ms = raw.ru_utime.tv_sec as f64 * 1000.0 + raw.ru_utime.tv_usec as f64 / 1000.0;
    let sys_ms = raw.ru_stime.tv_sec as f64 * 1000.0 + raw.ru_stime.tv_usec as f64 / 1000.0;
    let mut peak = raw.ru_maxrss as u64;
    #[cfg(target_os = "macos")]
    {
        peak /= 1024;
    }
    Usage {
        cpu_ms: user_ms + sys_ms,
        peak_rss_kib: peak,
    }
}

#[cfg(not(unix))]
#[derive(Clone, Copy)]
struct Usage {
    cpu_ms: f64,
    peak_rss_kib: u64,
}

#[cfg(not(unix))]
fn usage() -> Usage {
    Usage {
        cpu_ms: 0.0,
        peak_rss_kib: 0,
    }
}
