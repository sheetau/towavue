use super::*;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

#[test]
fn launch_payload_preserves_windows_paths_and_rejects_malformed_requests() {
    assert_eq!(decode(&encode(None).expect("Welcome")), Some(None));
    for path in [
        PathBuf::from(r"C:\media [one]\日本語 🎵.png"),
        PathBuf::from(r"\\server\share\movie.mp4"),
        PathBuf::from(OsString::from_wide(&[
            67, 58, 92, 0xd800, 46, 112, 110, 103,
        ])),
    ] {
        assert_eq!(
            decode(&encode(Some(&path)).expect("path")),
            Some(Some(path))
        );
    }
    for units in [
        vec![],
        vec![0, 1],
        vec![2],
        vec![1],
        vec![1, 67, 58, 92, 0],
        vec![1, 120],
    ] {
        assert!(decode(&units).is_none());
    }
    assert!(encode(Some(Path::new("relative.png"))).is_err());
    assert!(encode(Some(Path::new(&format!("C:\\{}", "x".repeat(MAX_UNITS))))).is_err());
}

#[test]
fn launch_forwarding_is_local_acknowledged_and_reusable() {
    const CHILD: &str = "TOWAVUE_TEST_LAUNCH_IDENTITY";
    let executable = std::env::current_exe()
        .expect("test executable")
        .canonicalize()
        .expect("canonical");
    let path = PathBuf::from(r"C:\launch [test]\日本語.png");
    if let Ok(identity) = std::env::var(CHILD) {
        if std::env::var_os("TOWAVUE_TEST_LAUNCH_OWNER_EXIT").is_some() {
            let owner = start_or_forward(&identity, &executable, None, Box::new(|_| {}))
                .expect("helper owner");
            assert!(matches!(owner, LaunchRole::Primary(_)));
            // Deliberately skip every destructor, including the receiver and marker.
            std::process::exit(0);
        }
        let result = start_or_forward(
            &identity,
            &executable,
            Some(&path),
            Box::new(|_| panic!("not the owner")),
        );
        if std::env::var_os("TOWAVUE_TEST_LAUNCH_REJECT").is_some() {
            assert!(result.is_err());
        } else {
            match result {
                Ok(LaunchRole::Forwarded) => {}
                Ok(LaunchRole::Primary(_)) => panic!("child unexpectedly owns the receiver"),
                Err(error) => panic!("forward failed: {error}"),
            }
        }
        return;
    }
    let identity = format!(
        "towavue-launch-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    );
    let (sent, received) = mpsc::channel();
    let server = start_or_forward(
        &identity,
        &executable,
        None,
        Box::new(move |request| {
            sent.send(request).expect("test receiver");
        }),
    )
    .expect("owner");
    assert!(matches!(server, LaunchRole::Primary(_)));
    assert!(
        start_or_forward(
            &identity,
            Path::new(r"C:\wrong-executable.exe"),
            Some(&path),
            Box::new(|_| {})
        )
        .is_err()
    );
    assert!(
        received.try_recv().is_err(),
        "identity mismatch was not delivered"
    );
    let child = |reject: bool| {
        let mut command = Command::new(&executable);
        command
            .args([
                "--exact",
                "launch::tests::launch_forwarding_is_local_acknowledged_and_reusable",
                "--nocapture",
            ])
            .env(CHILD, &identity)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW.0);
        if reject {
            command.env("TOWAVUE_TEST_LAUNCH_REJECT", "1");
        }
        command.spawn().expect("owned child")
    };
    let mut clients = vec![child(false), child(false)];
    for _ in 0..2 {
        let request = match received.recv_timeout(Duration::from_secs(5)) {
            Ok(request) => request,
            Err(error) => {
                for child in clients.drain(..) {
                    let output = child.wait_with_output().expect("child diagnostics");
                    eprintln!(
                        "{} {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                panic!("forwarded request: {error}");
            }
        };
        assert_eq!(request.path, Some(path.clone()));
        request.acknowledge(true);
    }
    for child in clients {
        let output = child.wait_with_output().expect("child exit");
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let rejected = child(true);
    received
        .recv_timeout(Duration::from_secs(5))
        .expect("rejected request")
        .acknowledge(false);
    assert!(
        rejected
            .wait_with_output()
            .expect("rejected exit")
            .status
            .success()
    );
    assert!(
        received.try_recv().is_err(),
        "no retry or duplicate delivery"
    );
    let timed_out = child(true);
    let unanswered = received
        .recv_timeout(Duration::from_secs(5))
        .expect("unanswered request");
    assert!(
        timed_out
            .wait_with_output()
            .expect("timeout exit")
            .status
            .success()
    );
    drop(unanswered);
    assert!(
        received.try_recv().is_err(),
        "timeout did not replay the request"
    );
    drop(server);
    let replacement =
        start_or_forward(&identity, &executable, None, Box::new(|_| {})).expect("marker released");
    assert!(matches!(replacement, LaunchRole::Primary(_)));
    drop(replacement);
    let output = Command::new(&executable)
        .args([
            "--exact",
            "launch::tests::launch_forwarding_is_local_acknowledged_and_reusable",
            "--nocapture",
        ])
        .env(CHILD, &identity)
        .env("TOWAVUE_TEST_LAUNCH_OWNER_EXIT", "1")
        .creation_flags(CREATE_NO_WINDOW.0)
        .output()
        .expect("owner exit helper");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recovered = start_or_forward(&identity, &executable, None, Box::new(|_| {}))
        .expect("kernel releases abandoned marker");
    assert!(matches!(recovered, LaunchRole::Primary(_)));
}
