//! Black-box lifecycle tests: run the real `cp-console-server` binary on a
//! private socket and drive it over the JSON-line protocol.
//!
//! Each test pins one regression: SIGTERM cleanup, process-group kills,
//! duplicate keys, signal exit codes, a second server on a live socket, and
//! `send` to a child that never reads its stdin.

use nix as _;
use rlimit as _;
use serde as _;
use serde_json as _;
use signal_hook as _;

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use serde_json::{Value, json};

    /// A server process on its own socket in a fresh temp dir.
    struct Server {
        /// The running `cp-console-server` process.
        child: Child,
        /// Scratch dir holding the socket, pid file and logs.
        dir: PathBuf,
    }

    /// Field `key` of a JSON reply, `Null` when absent.
    fn field<'reply>(reply: &'reply Value, key: &str) -> &'reply Value {
        reply.get(key).unwrap_or(&Value::Null)
    }

    /// `true` when the reply carries `"ok": true`.
    fn is_ok(reply: &Value) -> bool {
        field(reply, "ok").as_bool() == Some(true)
    }

    impl Server {
        /// Start a server and wait until it answers `ping`.
        fn start(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("cpcs-{name}-{}", std::process::id()));
            drop(std::fs::remove_dir_all(&dir));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let child = spawn_server(&dir.join("s.sock"));
            let server = Self { child, dir };
            assert!(wait_until(|| server.try_request(&json!({"cmd": "ping"})).is_some()), "server never came up");
            server
        }

        /// Socket path of this server.
        fn sock(&self) -> PathBuf {
            self.dir.join("s.sock")
        }

        /// One request/response round trip, `None` if the server is unreachable.
        fn try_request(&self, req: &Value) -> Option<Value> {
            let stream = UnixStream::connect(self.sock()).ok()?;
            let mut writer = stream.try_clone().ok()?;
            writeln!(writer, "{req}").ok()?;
            let mut line = String::new();
            let _n: usize = BufReader::new(stream).read_line(&mut line).ok()?;
            serde_json::from_str(&line).ok()
        }

        /// Like [`Self::try_request`], panicking when the server is unreachable.
        fn request(&self, req: &Value) -> Value {
            self.try_request(req).expect("server reachable")
        }

        /// Create a session running `command`; returns the raw response.
        fn create(&self, key: &str, command: &str) -> Value {
            let log = self.dir.join(format!("{key}.log"));
            self.request(&json!({"cmd": "create", "key": key, "command": command, "log_path": log}))
        }

        /// Start a session whose `sh` backgrounds a long sleep, and return the
        /// sleep's PID (a grandchild of the server).
        fn grandchild(&self, key: &str) -> i32 {
            let marker = self.dir.join(format!("{key}.pid"));
            let script = format!("sleep 300 & echo $! > {}; wait", marker.display());
            assert!(is_ok(&self.create(key, &script)), "create grandchild session");
            assert!(wait_until(|| std::fs::read_to_string(&marker).is_ok_and(|s| s.ends_with('\n'))), "no pid marker");
            std::fs::read_to_string(&marker).expect("marker").trim().parse().expect("pid")
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            if let Ok(raw) = i32::try_from(self.child.id()) {
                let _: Option<()> = kill(Pid::from_raw(raw), Signal::SIGTERM).ok();
            }
            drop(self.child.wait());
            drop(std::fs::remove_dir_all(&self.dir));
        }
    }

    /// Launch the binary under test on `sock`.
    fn spawn_server(sock: &Path) -> Child {
        Command::new(env!("CARGO_BIN_EXE_cp-console-server")).arg(sock).spawn().expect("spawn server")
    }

    /// Poll `cond` every 10 ms for up to 5 s.
    fn wait_until<F>(mut cond: F) -> bool
    where
        F: FnMut() -> bool,
    {
        let deadline = Instant::now().checked_add(Duration::from_secs(5)).expect("deadline");
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// `true` once `pid` no longer exists (zombies are reaped by init promptly).
    fn gone(pid: i32) -> bool {
        wait_until(|| kill(Pid::from_raw(pid), None).is_err())
    }

    #[test]
    fn sigterm_kills_process_tree_and_removes_socket() {
        let mut server = Server::start("sigterm");
        let grandchild = server.grandchild("c_1");
        let raw = i32::try_from(server.child.id()).expect("pid fits");
        kill(Pid::from_raw(raw), Signal::SIGTERM).expect("signal server");
        let _status = server.child.wait().expect("server exits");
        assert!(!server.sock().exists(), "socket must be removed on SIGTERM");
        assert!(gone(grandchild), "grandchild must die with the server");
    }

    #[test]
    fn kill_reaches_grandchildren() {
        let server = Server::start("killtree");
        let grandchild = server.grandchild("c_1");
        assert!(is_ok(&server.request(&json!({"cmd": "kill", "key": "c_1"}))), "kill ok");
        assert!(gone(grandchild), "kill must signal the whole process group");
    }

    #[test]
    fn duplicate_running_key_is_rejected() {
        let server = Server::start("dupkey");
        assert!(is_ok(&server.create("c_1", "sleep 30")), "first create ok");
        assert!(!is_ok(&server.create("c_1", "sleep 30")), "a running key must not be replaced");
    }

    #[test]
    fn signal_death_reports_128_plus_signal() {
        let server = Server::start("sigexit");
        assert!(is_ok(&server.create("c_1", "kill -9 $$")), "create ok");
        let exit_code = || field(&server.request(&json!({"cmd": "status", "key": "c_1"})), "exit_code").as_i64();
        assert!(wait_until(|| exit_code().is_some()), "session never exited");
        assert_eq!(exit_code(), Some(137i64), "SIGKILL must read 137");
    }

    #[test]
    fn second_server_leaves_live_socket_alone() {
        let server = Server::start("second");
        let status = spawn_server(&server.sock()).wait().expect("second server exits");
        assert!(status.success(), "second server exits cleanly");
        assert!(is_ok(&server.request(&json!({"cmd": "ping"}))), "first server still reachable");
    }

    #[test]
    fn send_to_child_not_reading_stdin_times_out() {
        let server = Server::start("sendhang");
        assert!(is_ok(&server.create("c_1", "sleep 30")), "create ok");
        let flood = "a".repeat(512 * 1024);
        let reply = server.request(&json!({"cmd": "send", "key": "c_1", "input": flood}));
        assert!(!is_ok(&reply), "send must fail instead of hanging");
        let error = field(&reply, "error").as_str().unwrap_or_default();
        assert!(error.contains("timed out"), "timeout error: {reply}");
    }
}
