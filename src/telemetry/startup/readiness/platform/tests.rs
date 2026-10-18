use std::{
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

// Exercise the real wrapper, waiting timer, and production stderr-forwarding
// path in a subprocess. A Tokio timeout cannot interrupt a blocking syscall.
fn wrapper_with_pipe(full: bool) -> Vec<u8> {
    let mut fds = [-1; 2];
    // SAFETY: pipe initializes two owned descriptors on success.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let (mut reader, mut writer) =
        unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) };
    // SAFETY: all fcntl calls use the live owned writer and supported flags.
    let original = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) };
    assert!(original >= 0);
    if full {
        assert_eq!(
            unsafe {
                libc::fcntl(
                    writer.as_raw_fd(),
                    libc::F_SETFL,
                    original | libc::O_NONBLOCK,
                )
            },
            0
        );
        loop {
            match writer.write(&[b'x'; 4096]) {
                Ok(n) => assert!(n > 0),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => panic!("synthetic pipe fill failed"),
            }
        }
        assert_eq!(
            unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_SETFL, original) },
            0
        );
    }
    let before_spawn = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) };
    assert_eq!(before_spawn & libc::O_NONBLOCK, 0);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tor_ingress::tests::actual_export_stall_does_not_delay_running_or_engine_failure",
            "--nocapture",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(writer.try_clone().unwrap()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("readiness wrapper blocked on platform output");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    assert_eq!(
        unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) },
        before_spawn,
        "emitter must not alter stderr's shared open-file description"
    );
    drop(writer);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn full_stderr_pipe_cannot_block_actual_readiness_wrapper() {
    let bytes = wrapper_with_pipe(true);
    assert!(
        bytes.iter().all(|byte| *byte == b'x'),
        "full output must be dropped"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_nonroot_writable_fifo_captures_real_readiness_record() {
    // This positive qualification deliberately cannot pass as a Darwin no-op.
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "qualify the shipping nonroot path"
    );
    let bytes = wrapper_with_pipe(false);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("event=possums.startup_readiness outcome=waiting"));
}
