use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use paint_plugin_bridge::protocol::{
    BgraImage, HEADER_LEN, MAGIC, MAX_PAYLOAD_LEN, MessageKind, PROTOCOL_VERSION,
};
use paint_plugin_bridge::{BrokerOptions, HostError, PluginHost};

fn main() {
    if let Err(error) = run() {
        eprintln!("CROSS-LANGUAGE CONFORMANCE FAILED: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let host_dll = PathBuf::from(arguments.next().ok_or_else(|| {
        "usage: paint-plugin-conformance <Paint.PluginHost.dll> [dotnet-path]".to_owned()
    })?);
    let dotnet = arguments
        .next()
        .unwrap_or_else(|| std::ffi::OsString::from("dotnet"));
    if arguments.next().is_some() {
        return Err("too many arguments".to_owned());
    }
    if !host_dll.is_file() {
        return Err(format!(
            "the real .NET host DLL does not exist: {}",
            host_dll.display()
        ));
    }

    run_check("valid copied BGRA output", || {
        let mut host = spawn(&dotnet, &host_dll, 7, Duration::from_secs(5), 0)?;
        let output = host
            .apply(7, "Invert", &source()?)
            .map_err(|error| error.to_string())?;
        check(
            output.pixels() == expected_output(),
            "invert output differed",
        )?;
        check(
            output
                .pixels()
                .iter()
                .skip(3)
                .step_by(4)
                .copied()
                .collect::<Vec<_>>()
                == vec![0, 1, 127, 255],
            "alpha was not preserved",
        )?;
        host.close().map_err(|error| error.to_string())
    })?;
    run_check("stale document generation", || {
        let mut host = spawn(&dotnet, &host_dll, 7, Duration::from_secs(5), 0)?;
        host.advance_generation(8)
            .map_err(|error| error.to_string())?;
        let result = host.apply(7, "Invert", &source()?);
        check(
            matches!(
                result,
                Err(HostError::StaleGeneration {
                    expected: 8,
                    actual: 7
                })
            ),
            format!("stale request was not rejected: {result:?}"),
        )?;
        host.close().map_err(|error| error.to_string())
    })?;
    run_check(
        "cooperative cancellation and late-response rejection",
        || {
            let mut host = spawn(&dotnet, &host_dll, 1, Duration::from_secs(5), 5_000)?;
            let request_id = host
                .start_apply(1, "Invert", &source()?)
                .map_err(|error| error.to_string())?;
            host.cancel(request_id).map_err(|error| error.to_string())?;
            check(
                matches!(
                    host.wait_for_apply(request_id),
                    Err(HostError::Cancelled { .. })
                ),
                "cancelled request did not return Cancelled",
            )?;
            host.close().map_err(|error| error.to_string())?;
            check(
                host.rejected_stale_responses() == 1,
                format!(
                    "expected one consumed late cancellation response, got {}",
                    host.rejected_stale_responses()
                ),
            )
        },
    )?;
    run_check("request timeout kills the real .NET child", || {
        let mut host = spawn(&dotnet, &host_dll, 1, Duration::from_millis(100), 5_000)?;
        let request_id = host
            .start_apply(1, "Invert", &source()?)
            .map_err(|error| error.to_string())?;
        check(
            matches!(
                host.wait_for_apply(request_id),
                Err(HostError::Timeout { .. })
            ),
            "delayed request did not time out",
        )?;
        check(
            !host.is_running().map_err(|error| error.to_string())?,
            "timed-out .NET child is still running",
        )
    })?;
    run_check("forced child crash during an active request", || {
        let mut host = spawn(&dotnet, &host_dll, 1, Duration::from_secs(5), 5_000)?;
        let request_id = host
            .start_apply(1, "Invert", &source()?)
            .map_err(|error| error.to_string())?;
        host.terminate().map_err(|error| error.to_string())?;
        check(
            matches!(
                host.wait_for_apply(request_id),
                Err(HostError::ChildExited { .. } | HostError::TransportDisconnected)
            ),
            "forced crash was not reported as a transport/lifecycle failure",
        )?;
        check(
            !host.is_running().map_err(|error| error.to_string())?,
            "crashed .NET child is still running",
        )
    })?;
    run_check("malformed frame", || {
        raw_rejection(
            &dotnet,
            &host_dll,
            PROTOCOL_VERSION,
            MessageKind::Apply,
            1,
            &[0],
        )
    })?;
    run_check("oversized declared frame", || {
        raw_rejection(
            &dotnet,
            &host_dll,
            PROTOCOL_VERSION,
            MessageKind::Apply,
            (MAX_PAYLOAD_LEN + 1) as u32,
            &[],
        )
    })?;
    run_check("wrong protocol version", || {
        raw_rejection(
            &dotnet,
            &host_dll,
            PROTOCOL_VERSION + 1,
            MessageKind::Hello,
            0,
            &[],
        )
    })?;

    println!("Cross-language conformance: 8 passed, 0 failed, 0 skipped.");
    Ok(())
}

fn run_check<F>(name: &str, check_fn: F) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    check_fn().map_err(|error| format!("{name}: {error}"))?;
    println!("PASS: {name}");
    Ok(())
}

fn spawn(
    dotnet: &std::ffi::OsStr,
    host_dll: &Path,
    generation: u64,
    timeout: Duration,
    diagnostic_delay_ms: u64,
) -> Result<PluginHost, String> {
    let mut command = Command::new(dotnet);
    command.arg(host_dll);
    if diagnostic_delay_ms > 0 {
        command
            .arg("--diagnostic-delay-ms")
            .arg(diagnostic_delay_ms.to_string());
    }
    PluginHost::spawn(
        command,
        BrokerOptions::new(timeout).with_initial_generation(generation),
    )
    .map_err(|error| error.to_string())
}

fn raw_rejection(
    dotnet: &std::ffi::OsStr,
    host_dll: &Path,
    version: u16,
    kind: MessageKind,
    payload_length: u32,
    payload: &[u8],
) -> Result<(), String> {
    let mut command = Command::new(dotnet);
    let mut child = command
        .arg(host_dll)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut header = [0_u8; HEADER_LEN];
    header[..4].copy_from_slice(&MAGIC);
    header[4..6].copy_from_slice(&version.to_le_bytes());
    header[6] = kind as u8;
    header[8..12].copy_from_slice(&payload_length.to_le_bytes());
    let write_result = {
        let stdin = child.stdin.as_mut().ok_or("child stdin was not piped")?;
        stdin
            .write_all(&header)
            .and_then(|()| stdin.write_all(payload))
    };
    child.stdin.take();
    if let Err(error) = write_result {
        reap_after_failure(&mut child);
        return Err(format!("could not write raw frame: {error}"));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "invalid frame was accepted with exit status {status}"
                ));
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                reap_after_failure(&mut child);
                return Err("host did not reject the raw frame within 10 seconds".to_owned());
            }
            Err(error) => {
                reap_after_failure(&mut child);
                return Err(format!("could not wait for raw host: {error}"));
            }
        }
    }
}

fn reap_after_failure(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn source() -> Result<BgraImage, String> {
    BgraImage::new(
        2,
        2,
        vec![
            0, 1, 2, 0, 10, 20, 30, 1, 100, 150, 200, 127, 250, 251, 252, 255,
        ],
    )
    .map_err(|error| error.to_string())
}

const fn expected_output() -> &'static [u8] {
    &[
        255, 254, 253, 0, 245, 235, 225, 1, 155, 105, 55, 127, 5, 4, 3, 255,
    ]
}

fn check(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
