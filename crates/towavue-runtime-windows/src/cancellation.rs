use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Default)]
struct State {
    cancelled: AtomicBool,
    child: Mutex<Option<Child>>,
}

/// Cancellation for one task and its sequential child processes.
#[derive(Clone, Default)]
pub struct Cancellation(Arc<State>);

impl Cancellation {
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Relaxed)
    }

    pub(crate) fn cancel(&self) {
        let mut child = self.0.child.lock().expect("preview child");
        self.0.cancelled.store(true, Ordering::Relaxed);
        if let Some(child) = child.as_mut()
            && let Err(error) = child.kill()
        {
            eprintln!("towavue: could not cancel preview child: {error}");
        }
    }

    pub(crate) fn output(&self, command: Command) -> io::Result<Output> {
        let (status, stdout, stderr) = self.read_output(command, |reader| {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).map(|_| bytes)
        })?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    pub(crate) fn read_output<T: Send>(
        &self,
        mut command: Command,
        read_stdout: impl FnOnce(&mut dyn Read) -> io::Result<T> + Send,
    ) -> io::Result<(ExitStatus, T, Vec<u8>)> {
        let (mut stdout, mut stderr) = {
            let mut active = self.0.child.lock().expect("preview child");
            if self.is_cancelled() {
                return Err(io::ErrorKind::Interrupted.into());
            }
            assert!(active.is_none(), "one child per preview task");
            // Register under the cancellation lock so close cannot miss a just-spawned child.
            let mut child = command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            let pipes = (
                child.stdout.take().expect("stdout"),
                child.stderr.take().expect("stderr"),
            );
            *active = Some(child);
            pipes
        };
        thread::scope(|scope| {
            let output = scope.spawn(move || read_stdout(&mut stdout));
            let diagnostics = scope.spawn(move || {
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let count = stderr.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.len() > 16_384 {
                        bytes.drain(..bytes.len() - 16_384);
                    }
                }
                Ok::<_, io::Error>(bytes)
            });
            let status = loop {
                let mut active = self.0.child.lock().expect("preview child");
                let child = active.as_mut().expect("running child");
                match child.try_wait() {
                    Ok(Some(status)) => {
                        active.take();
                        break status;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        active.take();
                        return Err(error);
                    }
                }
                drop(active);
                thread::sleep(Duration::from_millis(20));
            };
            Ok((
                status,
                output.join().expect("preview output reader")?,
                diagnostics.join().expect("preview diagnostic reader")?,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;
    use crate::LatestTask;

    fn ffmpeg_command() -> Command {
        let executable = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
        let mut command = Command::new(executable);
        <Command as std::os::windows::process::CommandExt>::creation_flags(
            &mut command,
            0x0800_0000,
        );
        command
    }

    #[test]
    fn process_pipes_are_drained_and_only_the_diagnostic_tail_is_kept() {
        const CHILD: &str = "TOWAVUE_DIAGNOSTIC_FIXTURE";
        if std::env::var_os(CHILD).is_some() {
            use std::io::Write;
            std::io::stderr()
                .write_all(&vec![b'x'; 100_000])
                .expect("diagnostics");
            std::io::stderr()
                .write_all(b"diagnostic tail")
                .expect("tail");
            std::io::stdout()
                .write_all(&vec![b'y'; 100_000])
                .expect("output");
            return;
        }
        let mut command = Command::new(std::env::current_exe().expect("test executable"));
        command.args(["--exact", "cancellation::tests::process_pipes_are_drained_and_only_the_diagnostic_tail_is_kept", "--nocapture"])
            .env(CHILD, "1");
        <Command as std::os::windows::process::CommandExt>::creation_flags(
            &mut command,
            0x0800_0000,
        );
        let output = Cancellation::default()
            .output(command)
            .expect("pipe fixture");
        assert!(output.status.success());
        assert!(output.stdout.len() >= 100_000);
        assert_eq!(output.stderr.len(), 16_384);
        assert!(output.stderr.ends_with(b"diagnostic tail"));
    }

    #[test]
    fn replacing_clearing_and_dropping_tasks_stop_their_owned_child() {
        for action in 0..3 {
            let mut worker = Some(LatestTask::new("cancel-preview-test").expect("worker"));
            let (started_tx, started_rx) = mpsc::channel();
            let (finished_tx, finished_rx) = mpsc::channel();
            worker
                .as_ref()
                .expect("worker")
                .submit(move |cancellation| {
                    started_tx
                        .send(cancellation.clone())
                        .expect("started token");
                    let mut command = ffmpeg_command();
                    command.args([
                        "-v",
                        "error",
                        "-re",
                        "-f",
                        "lavfi",
                        "-i",
                        "sine=duration=60",
                        "-f",
                        "null",
                        "-",
                    ]);
                    finished_tx
                        .send(cancellation.output(command))
                        .expect("child result");
                });
            let cancellation = started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("task starts");
            let deadline = Instant::now() + Duration::from_secs(5);
            while cancellation.0.child.lock().expect("child").is_none() {
                assert!(Instant::now() < deadline, "FFmpeg child did not start");
                thread::sleep(Duration::from_millis(5));
            }
            let (next_tx, next_rx) = mpsc::channel();
            match action {
                0 => worker.as_ref().expect("worker").submit(move |next| {
                    let mut command = ffmpeg_command();
                    command.arg("-version");
                    next_tx.send(next.output(command)).expect("fresh result");
                }),
                1 => worker.as_ref().expect("worker").clear(),
                _ => drop(worker.take()),
            }
            let result = finished_rx.recv_timeout(Duration::from_secs(2));
            if result.is_err() {
                // Reap the fixture even when the cancellation regression fails.
                if let Some(child) = cancellation.0.child.lock().expect("child").as_mut() {
                    let _ = child.kill();
                }
                let _ = finished_rx.recv_timeout(Duration::from_secs(5));
            }
            assert!(
                !result
                    .expect("cancelled child completes promptly")
                    .expect("process output")
                    .status
                    .success()
            );
            assert!(cancellation.0.child.lock().expect("child").is_none());
            assert_eq!(
                cancellation
                    .output(Command::new("must-not-start.exe"))
                    .expect_err("cancelled before spawn")
                    .kind(),
                io::ErrorKind::Interrupted
            );
            if action == 0 {
                assert!(
                    next_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("next task runs")
                        .expect("fresh output")
                        .status
                        .success()
                );
            }
        }
    }
}
