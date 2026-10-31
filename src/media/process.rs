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
    codec: Option<Codec>,
    video: Option<super::vp8::Guard>,
    pid: u32,
    pub input: Option<ChildStdin>,
    pub output: Option<ChildStdout>,
    stop: Option<oneshot::Sender<()>>,
    finished: Option<JoinHandle<Result<Exit>>>,
}
impl Pipeline {
    pub(super) fn decoder(mut self, codec: Codec) -> Self {
        self.codec = Some(codec);
        self.video = (codec == Codec::Vp8).then(super::vp8::Guard::default);
        self
    }
    /// Peer RTP enters a native decoder only through this typed, bounded input.
    /// `input` remains the low-level trusted interoperability pipe.
    pub async fn write_packet(&mut self, packet: &Packet) -> Result<()> {
        if self.codec != Some(packet.codec()) {
            return Err(Error::Protocol);
        }
        if let Some(guard) = &mut self.video {
            let packets = guard.push(Packet::parse(packet.codec(), packet.bytes().to_vec())?)?;
            let input = self.input.as_mut().ok_or(Error::Protocol)?;
            for packet in packets {
                packet.write(input).await?;
            }
            Ok(())
        } else {
            packet
                .write(self.input.as_mut().ok_or(Error::Protocol)?)
                .await
        }
    }
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
            codec: None,
            video: None,
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
    /// Deliver EOS to an input-only playback adapter and collect its real exit.
    /// Capture/duplex pipelines need shutdown or an explicitly drained output;
    /// silently dropping an unread output here could deadlock the child.
    pub async fn finish_input(self) -> Result<Exit> {
        self.finish_input_or_cancel(std::future::pending()).await
    }
    /// Cancellation/revocation wins over a simultaneously ready decoder exit.
    /// Always stop and reap before surfacing a cancellation error.
    pub async fn finish_input_or_cancel(
        mut self,
        cancel: impl std::future::Future<Output = Result<()>>,
    ) -> Result<Exit> {
        if self.input.is_none() || self.output.is_some() {
            return Err(Error::Configuration(
                "graceful native EOS requires an input-only playback pipeline",
            ));
        }
        self.input.take();
        let cancellation = tokio::select! {
            biased;
            cancellation = cancel => cancellation,
            exit = tokio::time::timeout(config::DEFAULT_TIMEOUT, self.finished()) => match exit {
                Ok(exit) => return exit,
                Err(_) => Err(Error::Timeout),
            },
        };
        let exit = self.shutdown().await;
        cancellation?;
        exit
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

    #[tokio::test]
    async fn native_vp8_adapter_rejects_oversized_keyframe_before_child_input() {
        // A byte-echo child makes any accidental decoder input observable;
        // no codec, capture device, display or physical output is opened.
        let mut sink = Pipeline::spawn("cat", vec![], true, true)
            .unwrap()
            .decoder(Codec::Vp8);
        let packet = Packet::parse(
            Codec::Vp8,
            vec![
                0x80,
                0x80 | 97,
                0,
                1,
                0,
                0,
                0,
                1,
                0,
                0,
                0,
                1,
                0x10,
                0,
                0,
                0,
                0x9d,
                1,
                0x2a,
                0xff,
                0x3f,
                0xff,
                0x3f,
            ],
        )
        .unwrap();
        assert!(matches!(
            sink.write_packet(&packet).await,
            Err(Error::Protocol)
        ));
        sink.input.take();
        let mut received = Vec::new();
        tokio::time::timeout(
            config::DEFAULT_TIMEOUT,
            sink.output.as_mut().unwrap().read_to_end(&mut received),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            received.is_empty(),
            "oversized compressed header reached the native decoder pipe"
        );
        assert!(sink.finished().await.unwrap().success);
    }

    #[tokio::test]
    async fn native_drain_ready_revocation_dominates_eof_and_pending_drain_is_reaped() {
        // Neither subprocess opens a source or an output device.
        for program in ["cat", "sleep"] {
            let args = if program == "sleep" {
                vec!["30".into()]
            } else {
                vec![]
            };
            let pipeline = Pipeline::spawn(program, args, true, false).unwrap();
            let pid = pipeline.id();
            let cancelled = pipeline
                .finish_input_or_cancel(async {
                    if program == "sleep" {
                        tokio::task::yield_now().await;
                    }
                    Err(Error::Denied)
                })
                .await;
            assert!(matches!(cancelled, Err(Error::Denied)));
            assert!(
                !std::path::Path::new(&format!("/proc/{pid}")).exists(),
                "revoked drain must reap its native output child"
            );
        }
    }

    #[tokio::test]
    async fn native_revocation_during_eos_drain_stops_and_reaps_before_returning() {
        let pipeline = Pipeline::spawn("sleep", vec!["30".into()], true, false).unwrap();
        let pid = pipeline.id();
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let draining = pipeline.finish_input_or_cancel(async {
            cancelled.await.map_err(|_| Error::Cancelled)?;
            Err(Error::Denied)
        });
        tokio::pin!(draining);
        tokio::select! {
            biased;
            result = &mut draining => panic!("test child must still be draining: {}", result.is_ok()),
            _ = tokio::task::yield_now() => {},
        }
        cancel.send(()).unwrap();
        assert!(matches!(
            tokio::time::timeout(config::DEFAULT_TIMEOUT, &mut draining)
                .await
                .unwrap(),
            Err(Error::Denied)
        ));
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "cancelled decoder must be reaped"
        );
    }

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
