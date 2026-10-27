use std::process::Command;
use std::time::Duration;

use paint_plugin_bridge::protocol::BgraImage;
use paint_plugin_bridge::{BrokerOptions, HostError, PluginHost};

fn spawn(mode: &str, timeout: Duration) -> PluginHost {
    let mut command = Command::new(env!("CARGO_BIN_EXE_paint-plugin-fixture"));
    command.arg(mode);
    PluginHost::spawn(
        command,
        BrokerOptions::new(timeout).with_initial_generation(1),
    )
    .unwrap()
}

fn source() -> BgraImage {
    BgraImage::new(
        2,
        2,
        vec![
            0, 1, 2, 0, //
            10, 20, 30, 1, //
            100, 150, 200, 127, //
            250, 251, 252, 255,
        ],
    )
    .unwrap()
}

#[test]
fn real_child_inverts_bgra_and_preserves_alpha() {
    let mut host = spawn("invert", Duration::from_secs(5));
    assert_eq!(host.effects(), &["Invert"]);
    let output = host.apply(1, "Invert", &source()).unwrap();
    assert_eq!(output.width(), 2);
    assert_eq!(output.height(), 2);
    assert_eq!(output.stride(), 8);
    assert_eq!(
        output.pixels(),
        &[
            255, 254, 253, 0, //
            245, 235, 225, 1, //
            155, 105, 55, 127, //
            5, 4, 3, 255,
        ]
    );
    host.close().unwrap();
}

#[test]
fn stale_requests_and_stale_responses_are_rejected() {
    let mut host = spawn("invert", Duration::from_secs(5));
    host.advance_generation(8).unwrap();
    assert!(matches!(
        host.apply(7, "Invert", &source()),
        Err(HostError::StaleGeneration {
            expected: 8,
            actual: 7
        })
    ));
    host.close().unwrap();

    let mut stale_host = spawn("stale-response", Duration::from_secs(5));
    stale_host.advance_generation(7).unwrap();
    assert!(matches!(
        stale_host.apply(7, "Invert", &source()),
        Err(HostError::StaleResponse {
            expected: 7,
            actual: 6,
            ..
        })
    ));
}

#[test]
fn cancellation_returns_cancelled_instead_of_a_late_result() {
    let mut host = spawn("hang", Duration::from_secs(5));
    let id = host.start_apply(1, "Invert", &source()).unwrap();
    host.cancel(id).unwrap();
    assert!(matches!(
        host.wait_for_apply(id),
        Err(HostError::Cancelled { request_id }) if request_id == id
    ));
}

#[test]
fn request_timeout_kills_and_reaps_a_stuck_child() {
    let mut host = spawn("hang", Duration::from_millis(75));
    let id = host.start_apply(1, "Invert", &source()).unwrap();
    assert!(matches!(
        host.wait_for_apply(id),
        Err(HostError::Timeout { request_id, .. }) if request_id == id
    ));
    assert!(!host.is_running().unwrap());
}

#[test]
fn child_crash_is_reported_and_reaped() {
    let mut host = spawn("crash", Duration::from_secs(5));
    assert!(matches!(
        host.apply(1, "Invert", &source()),
        Err(HostError::ChildExited { code: Some(42) })
    ));
    assert!(!host.is_running().unwrap());
}

#[test]
fn malformed_child_output_is_a_fatal_protocol_error() {
    let mut host = spawn("malformed-output", Duration::from_secs(5));
    assert!(matches!(
        host.apply(1, "Invert", &source()),
        Err(HostError::Protocol(_))
    ));
    assert!(!host.is_running().unwrap());
}

#[test]
fn output_dimension_mismatch_is_fatal() {
    let mut host = spawn("wrong-dimensions", Duration::from_secs(5));
    assert!(matches!(
        host.apply(1, "Invert", &source()),
        Err(HostError::DimensionMismatch {
            expected_width: 2,
            expected_height: 2,
            actual_width: 1,
            actual_height: 1,
        })
    ));
    assert!(!host.is_running().unwrap());
}

#[test]
fn handshake_crash_is_not_a_successful_spawn() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_paint-plugin-fixture"));
    command.arg("crash-before-ready");
    let result = PluginHost::spawn(command, BrokerOptions::new(Duration::from_secs(5)));
    assert!(matches!(result, Err(HostError::ChildExited { .. })));
}

#[test]
fn crash_exit_status_is_stable_across_pipe_eof_races() {
    for _ in 0..8 {
        let mut host = spawn("crash", Duration::from_secs(5));
        assert!(matches!(
            host.apply(1, "Invert", &source()),
            Err(HostError::ChildExited { code: Some(42) })
        ));
        assert!(!host.is_running().unwrap());

        let mut command = Command::new(env!("CARGO_BIN_EXE_paint-plugin-fixture"));
        command.arg("crash-before-ready");
        let result = PluginHost::spawn(command, BrokerOptions::new(Duration::from_secs(5)));
        assert!(matches!(
            result,
            Err(HostError::ChildExited { code: Some(42) })
        ));
    }
}
