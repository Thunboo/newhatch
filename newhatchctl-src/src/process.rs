use std::{
    ffi::OsStr,
    io::Read,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use std::fmt::Write as _;

const CAPTURE_LIMIT: usize = 8 * 1024 * 1024;
const LINE_LIMIT: usize = 16 * 1024;

fn command(executable: &OsStr, directory: &Path, args: &[String]) -> Command {
    let mut command = Command::new(executable);
    command
        .current_dir(directory)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Terminate the CLI and its Compose plugin together on cancellation.
        command.process_group(0);
    }
    command
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn capture_reader(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let length = reader.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        let remaining = (CAPTURE_LIMIT + 1).saturating_sub(result.len());
        result.extend_from_slice(&buffer[..length.min(remaining)]);
    }
    Ok(result)
}

pub fn capture(
    executable: &OsStr,
    directory: &Path,
    args: &[String],
    cancelled: &AtomicBool,
) -> Result<String> {
    let mut child = command(executable, directory, args)
        .spawn()
        .with_context(|| format!("Cannot run {}", executable.to_string_lossy()))?;
    let stdout = child.stdout.take().context("Missing command stdout")?;
    let stderr = child.stderr.take().context("Missing command stderr")?;
    let output = thread::spawn(move || capture_reader(stdout));
    let errors = thread::spawn(move || capture_reader(stderr));
    let started = Instant::now();
    let status = loop {
        if cancelled.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(30) {
            terminate(&mut child);
            let _ = output.join();
            let _ = errors.join();
            bail!("Command cancelled or timed out");
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                terminate(&mut child);
                return Err(error.into());
            }
        }
    };
    let stdout = output
        .join()
        .map_err(|_| anyhow::anyhow!("Output reader failed"))??;
    let stderr = errors
        .join()
        .map_err(|_| anyhow::anyhow!("Error reader failed"))??;
    if !status.success() {
        bail!("{}", clean_output(&String::from_utf8_lossy(&stderr)).trim());
    }
    if stdout.len() > CAPTURE_LIMIT {
        bail!("Command output exceeds 8 MiB");
    }
    String::from_utf8(stdout).context("Command returned invalid UTF-8")
}

pub struct Stream {
    child: Child,
    lines: Receiver<String>,
    finished: bool,
}

impl Stream {
    pub fn spawn(executable: &OsStr, directory: &Path, args: &[String]) -> Result<Self> {
        let mut child = command(executable, directory, args)
            .spawn()
            .with_context(|| format!("Cannot run {}", executable.to_string_lossy()))?;
        let (sender, lines) = mpsc::sync_channel(256);
        let stdout = child.stdout.take().context("Missing command stdout")?;
        let stderr = child.stderr.take().context("Missing command stderr")?;
        pump(stdout, sender.clone());
        pump(stderr, sender);
        Ok(Self {
            child,
            lines,
            finished: false,
        })
    }

    pub fn drain(&self) -> Vec<String> {
        self.lines.try_iter().collect()
    }

    pub fn poll(&mut self) -> Result<Option<ExitStatus>> {
        let result = self.child.try_wait()?;
        if result.is_some() {
            self.finished = true;
        }
        Ok(result)
    }

    pub fn execute(
        mut self,
        cancelled: &AtomicBool,
        mut observer: impl FnMut(String),
    ) -> Result<()> {
        loop {
            if cancelled.load(Ordering::Relaxed) {
                bail!("Operation cancelled");
            }
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(50)) {
                observer(line);
            }
            for line in self.drain() {
                observer(line);
            }
            if let Some(status) = self.poll()? {
                // Reader threads can still be draining the final pipe bytes.
                while let Ok(line) = self.lines.recv_timeout(Duration::from_millis(100)) {
                    observer(line);
                }
                if !status.success() {
                    bail!("Command failed ({status}); see output above");
                }
                return Ok(());
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if !self.finished {
            terminate(&mut self.child);
        }
    }
}

fn pump(mut reader: impl Read + Send + 'static, sender: SyncSender<String>) {
    thread::spawn(move || {
        let mut buffer = [0; 4096];
        let mut line = Vec::new();
        let mut truncated = false;
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) => {
                    let _ = sender.try_send(format!("Output error: {error}"));
                    break;
                }
            };
            for byte in &buffer[..count] {
                if *byte == b'\n' || *byte == b'\r' {
                    if !line.is_empty() || truncated {
                        let mut text = clean_output(&String::from_utf8_lossy(&line));
                        if truncated {
                            text.push_str(" [line truncated]");
                        }
                        // Backpressure bounds memory; stopping a view disconnects this pipe.
                        if sender.send(text).is_err() {
                            return;
                        }
                    }
                    line.clear();
                    truncated = false;
                } else if line.len() < LINE_LIMIT {
                    line.push(*byte);
                } else {
                    truncated = true;
                }
            }
        }
        if !line.is_empty() {
            let _ = sender.send(clean_output(&String::from_utf8_lossy(&line)));
        }
    });
}

fn clean_output(text: &str) -> String {
    // --no-color only controls Compose's own formatting. Container output can
    // still contain SGR colors, OSC hyperlinks, or other terminal sequences.
    // Expand tabs before stripping, since the ANSI parser discards other C0 codes.
    let plain = strip_ansi_escapes::strip_str(text.replace('\t', "    "));
    clean_text(&plain)
}

pub fn clean_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for character in text.chars() {
        if character == '\t' {
            result.push_str("    ");
        } else if character == '\n' || !character.is_control() {
            result.push(character);
        } else {
            let _ = write!(&mut result, "\\u{{{:x}}}", character as u32);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzer_color_codes_are_removed_without_losing_log_content() {
        let colored = "analyzer-1 | \x1b[2m2026-10-12T17:45:53Z\x1b[0m \x1b[32m INFO\x1b[0m \x1b[2mnewhatch_analyzer::api\x1b[0m: API listening address=127.0.0.1:3000";
        assert_eq!(clean_output(colored), "analyzer-1 | 2026-10-12T17:45:53Z  INFO newhatch_analyzer::api: API listening address=127.0.0.1:3000");
        assert_eq!(
            clean_output(
                "\x1b[33m WARN\x1b[0m\tНюхач: EOF\n\nCaused by:\n    unexpected end of file"
            ),
            " WARN    Нюхач: EOF\n\nCaused by:\n    unexpected end of file"
        );
    }

    #[test]
    fn output_strips_hyperlinks_and_cursor_sequences_but_preserves_literal_text() {
        assert_eq!(
            clean_output("\x1b]8;;https://example.com\x1b\\payload\x1b]8;;\x1b\\\x1b[2J done"),
            "payload done"
        );
        assert_eq!(
            clean_output("\x1b]0;terminal title\x07normal text [32m"),
            "normal text [32m"
        );
    }

    #[test]
    fn split_stream_reads_keep_colored_utf8_logs_readable() {
        struct Chunked(std::io::Cursor<Vec<u8>>);
        impl Read for Chunked {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let length = buffer.len().min(2);
                self.0.read(&mut buffer[..length])
            }
        }
        let (sender, receiver) = mpsc::sync_channel(4);
        pump(
            Chunked(std::io::Cursor::new(
                "\x1b[32mНюхач\x1b[0m\n\x1b[33mWARN\x1b[0m"
                    .as_bytes()
                    .to_vec(),
            )),
            sender,
        );
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            "Нюхач"
        );
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            "WARN"
        );
    }
}
