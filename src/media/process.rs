//! Owned subprocess lifetime and continuously drained, bounded diagnostics.
use super::*;
use std::{collections::VecDeque, process::Stdio};
use tokio::{
    process::{ChildStdin, ChildStdout, Command},
    sync::oneshot,
    task::JoinHandle,
};

pub struct Exit {
    pub success: bool,
    pub stopped: bool,
    pub code: Option<i32>,
    pub diagnostic: Zeroizing<Vec<u8>>,
}
pub struct Pipeline {
    pid: u32,
    pub input: Option<ChildStdin>,
    pub output: Option<ChildStdout>,
    stop: Option<oneshot::Sender<()>>,
    finished: Option<JoinHandle<Result<Exit>>>,
}
impl Pipeline {
    pub fn id(&self) -> u32 {
        self.pid
    }
    pub(super) fn start(args: Vec<String>, input: bool, output: bool) -> Result<Self> {
        Self::spawn(
            "gst-launch-1.0",
            std::iter::once("-q".to_owned()).chain(args).collect(),
            input,
            output,
        )
    }
    pub(super) fn inspect(plugin: &str) -> Result<Self> {
        Self::spawn(
            "gst-inspect-1.0",
            vec!["--exists".into(), plugin.into()],
            false,
            false,
        )
    }
    pub(super) fn spawn(
        program: &str,
        args: Vec<String>,
        input: bool,
        output: bool,
    ) -> Result<Self> {
        #[cfg(target_os = "linux")]
        let parent_gate = rustix::fs::open(
            "/proc/self",
            rustix::fs::OFlags::PATH | rustix::fs::OFlags::DIRECTORY,
            rustix::fs::Mode::empty(),
        )?;
        #[cfg(target_os = "linux")]
        let mut command = {
            use std::os::{fd::AsRawFd, unix::fs::PermissionsExt};
            let executable = if std::path::Path::new(program).is_absolute() {
                std::path::PathBuf::from(program)
            } else {
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                    .map(|directory| directory.join(program))
                    .find(|path| {
                        std::fs::metadata(path).is_ok_and(|metadata| {
                            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                        })
                    })
                    .ok_or_else(|| {
                        Error::Io(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "native media executable not found",
                        ))
                    })?
            };
            let executable = std::path::absolute(executable)?;
            // Inherit THIS parent's proc-directory FD, not a recyclable PID.
            // After arming pdeathsig, native exec resolves through that root:
            // an already-dead parent cannot start capture in the arming race.
            let guarded = format!(
                "/proc/self/fd/{}/root{}",
                parent_gate.as_raw_fd(),
                executable.display()
            );
            let mut bounded = Command::new("prlimit");
            bounded
                .arg(format!("--as={0}:{0}", config::MEDIA_PROCESS_ADDRESS_BYTES))
                .arg(format!("--data={0}:{0}", config::MEDIA_PROCESS_DATA_BYTES))
                .arg("--")
                .args(["setpriv", "--no-new-privs", "--pdeathsig", "KILL", "--"])
                .arg(guarded);
            bounded
        };
        #[cfg(not(target_os = "linux"))]
        let mut command = Command::new(program);
        command.args(args).kill_on_drop(true).env_clear();
        // Pass only native-session coordinates, never provider credentials or
        // arbitrary GST_DEBUG/plugin/program overrides inherited from the agent.
        for key in [
            "PATH",
            "HOME",
            "XDG_RUNTIME_DIR",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XAUTHORITY",
            "DBUS_SESSION_BUS_ADDRESS",
            "PULSE_SERVER",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command.env("GST_XINITTHREADS", "1");
        let mut child = command
            .stdin(if input { Stdio::piped() } else { Stdio::null() })
            .stdout(if output {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::piped())
            .spawn()?;
        #[cfg(target_os = "linux")]
        drop(parent_gate);
        let pid = child
            .id()
            .ok_or(Error::Node("native media child has no process identity"))?;
        let input = child.stdin.take();
        let output = child.stdout.take();
        let mut stderr = child
            .stderr
            .take()
            .ok_or(Error::Node("native media diagnostic pipe missing"))?;
        let mut drain = tokio::spawn(async move {
            let mut retained = VecDeque::with_capacity(config::MEDIA_DIAGNOSTIC_BYTES);
            let mut buffer = Zeroizing::new(vec![0; 2048]);
            loop {
                let count = stderr.read(&mut buffer).await?;
                if count == 0 {
                    break;
                }
                for byte in &buffer[..count] {
                    if retained.len() == config::MEDIA_DIAGNOSTIC_BYTES {
                        retained.pop_front();
                    }
                    retained.push_back(*byte);
                }
            }
            Ok::<_, Error>(Zeroizing::new(retained.into_iter().collect()))
        });
        let (stop, mut stopped) = oneshot::channel();
        let finished = tokio::spawn(async move {
            let mut requested_stop = false;
            let status = tokio::select! {
                status = child.wait() => status,
                _ = &mut stopped => async {
                    if child.try_wait()?.is_none() { child.start_kill()?; requested_stop = true; }
                    child.wait().await
                }.await,
            };
            let diagnostic = match tokio::time::timeout(config::DEFAULT_TIMEOUT, &mut drain).await {
                Ok(result) => {
                    result.map_err(|_| Error::Node("native media diagnostic task failed"))??
                }
                Err(_) => {
                    drain.abort();
                    let _ = drain.await;
                    return Err(Error::Node("native media diagnostic pipe did not close"));
                }
            };
            let status = status?;
            Ok(Exit {
                success: status.success(),
                stopped: requested_stop,
                code: status.code(),
                diagnostic,
            })
        });
        Ok(Self {
            pid,
            input,
            output,
            stop: Some(stop),
            finished: Some(finished),
        })
    }
    pub async fn finished(&mut self) -> Result<Exit> {
        let result = self
            .finished
            .as_mut()
            .ok_or(Error::Node("native media completion already collected"))?
            .await;
        self.finished.take();
        result.map_err(|_| Error::Node("native media owner task failed"))?
    }
    pub async fn shutdown(mut self) -> Result<Exit> {
        self.input.take();
        self.output.take();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        tokio::time::timeout(
            config::DEFAULT_TIMEOUT + config::QUIC_SHUTDOWN_GRACE,
            self.finished(),
        )
        .await
        .map_err(|_| Error::Timeout)?
    }
}
impl Drop for Pipeline {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::io::Write;
    use tokio::io::AsyncBufReadExt;

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "explicit native owner-SIGKILL gate; only synthetic audio, no physical capture"]
    async fn native_owner_sigkill_cannot_leave_a_capture_or_preview_child_alive() {
        const PROBE: &str = "KK_TEST_NATIVE_OWNER_PROBE";
        if std::env::var_os(PROBE).is_some() {
            let pipeline = Pipeline::start(
                ["audiotestsrc", "is-live=true", "!", "fakesink"]
                    .map(String::from)
                    .to_vec(),
                false,
                false,
            )
            .unwrap();
            // Wait for the real native executable, not just the setup wrappers.
            tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
                let mut timer = tokio::time::interval(std::time::Duration::from_millis(5));
                loop {
                    timer.tick().await;
                    if std::fs::read_link(format!("/proc/{}/exe", pipeline.id())).is_ok_and(
                        |path| {
                            path.file_name()
                                .is_some_and(|name| name == "gst-launch-1.0")
                        },
                    ) {
                        break;
                    }
                }
            })
            .await
            .unwrap();
            println!("NATIVEPID {}", pipeline.id());
            std::io::stdout().flush().unwrap();
            std::future::pending::<()>().await;
            drop(pipeline);
            return;
        }
        let mut owner = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "media::process::tests::native_owner_sigkill_cannot_leave_a_capture_or_preview_child_alive", "--ignored", "--nocapture"])
            .env(PROBE, "1").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).kill_on_drop(true).spawn().unwrap();
        let mut output = tokio::io::BufReader::new(owner.stdout.take().unwrap());
        let pid = tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
            loop {
                let mut line = String::new();
                assert!(
                    output.read_line(&mut line).await.unwrap() > 0,
                    "native owner probe exited before readiness"
                );
                if let Some((_, pid)) = line.split_once("NATIVEPID ") {
                    break pid.trim().parse::<i32>().unwrap();
                }
            }
        })
        .await
        .unwrap();
        let pid = rustix::process::Pid::from_raw(pid).unwrap();
        let pidfd = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).unwrap();
        owner.start_kill().unwrap();
        owner.wait().await.unwrap();
        // pidfd observes actual exit, including zombies, without PID-reuse or
        // fragile process-name scanning. No sleeps or signal-0 polling.
        let exited = tokio::task::spawn_blocking(move || {
            use rustix::event::{PollFd, PollFlags, Timespec, poll};
            let deadline = std::time::Instant::now() + config::DEFAULT_TIMEOUT;
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return false;
                }
                let timeout = Timespec::try_from(left).unwrap();
                let mut descriptors = [PollFd::new(&pidfd, PollFlags::IN)];
                match poll(&mut descriptors, Some(&timeout)) {
                    Ok(count) => return count > 0,
                    Err(rustix::io::Errno::INTR) => {}
                    Err(error) => panic!("native exit observation failed: {error}"),
                }
            }
        })
        .await
        .unwrap();
        assert!(exited, "native child survived owner SIGKILL");
    }
}
