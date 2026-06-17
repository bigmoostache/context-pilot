//! Subprocess transport: spawn an MCP server and exchange newline-delimited
//! JSON-RPC over its stdin/stdout.
//!
//! A dedicated reader thread parses each stdout line into an [`Incoming`] and
//! forwards it through an mpsc channel, so [`StdioTransport::recv`] can apply a
//! timeout via [`Receiver::recv_timeout`] without blocking on the pipe. This
//! mirrors the timeout pattern used elsewhere in the codebase for subprocess
//! I/O.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::error::McpError;
use crate::protocol::Incoming;

use super::Transport;

/// A spawned MCP server addressed over stdio.
#[derive(Debug)]
pub struct StdioTransport {
    /// Child process handle; killed on drop.
    child: Child,
    /// Writable end of the child's stdin.
    stdin: ChildStdin,
    /// Parsed inbound messages from the reader thread.
    rx: Receiver<Incoming>,
    /// Reader thread handle; joined on drop after the child is killed.
    reader: Option<JoinHandle<()>>,
}

impl StdioTransport {
    /// Spawn `command` with `args` and wire up stdio framing.
    ///
    /// stderr is inherited so server diagnostics surface in the host terminal.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Spawn`] if the process cannot start, or
    /// [`McpError::Transport`] if its stdio handles cannot be captured.
    pub fn spawn(command: &str, args: &[String]) -> Result<Self, McpError> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| McpError::Spawn(format!("{command}: {e}")))?;

        let stdin = child.stdin.take().ok_or_else(|| McpError::Transport("child stdin unavailable".to_owned()))?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Transport("child stdout unavailable".to_owned()))?;

        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let buf = BufReader::new(stdout);
            for line in buf.lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(msg) = serde_json::from_str::<Incoming>(&line) else {
                    // Skip frames we can't parse (e.g. non-JSON server logging).
                    continue;
                };
                if tx.send(msg).is_err() {
                    break; // Receiver dropped — client is gone.
                }
            }
        });

        Ok(Self { child, stdin, rx, reader: Some(reader) })
    }
}

impl Transport for StdioTransport {
    fn send_line(&mut self, json: &str) -> Result<(), McpError> {
        self.stdin.write_all(json.as_bytes()).map_err(|e| McpError::Transport(format!("write: {e}")))?;
        self.stdin.write_all(b"\n").map_err(|e| McpError::Transport(format!("write: {e}")))?;
        self.stdin.flush().map_err(|e| McpError::Transport(format!("flush: {e}")))
    }

    fn recv(&mut self, timeout_ms: u64) -> Result<Incoming, McpError> {
        match self.rx.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(msg) => Ok(msg),
            Err(RecvTimeoutError::Timeout) => Err(McpError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(McpError::Transport("server closed connection".to_owned())),
        }
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // Best-effort teardown: kill the child, then let the reader thread observe
        // EOF and exit. Errors are intentionally ignored — we're tearing down.
        let _ignored = self.child.kill();
        let _reaped = self.child.wait();
        if let Some(handle) = self.reader.take() {
            let _joined = handle.join();
        }
    }
}
