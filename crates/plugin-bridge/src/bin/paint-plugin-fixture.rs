use std::env;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use paint_plugin_bridge::protocol::{
    Message, ProtocolError, RemoteErrorCode, read_message, write_message,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("Rust plugin fixture failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), ProtocolError> {
    let mode = env::args().nth(1).unwrap_or_else(|| "invert".to_owned());
    if mode == "crash-before-ready" {
        std::process::exit(42);
    }

    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let hello = read_message(&mut input)?;
    let Message::Hello { request_id } = hello else {
        return Err(ProtocolError::InvalidPayload(
            "fixture expected Hello".to_owned(),
        ));
    };
    write_message(
        &mut output,
        &Message::Ready {
            request_id,
            effects: vec!["Invert".to_owned()],
        },
    )?;

    loop {
        match read_message(&mut input)? {
            Message::Apply {
                request_id,
                generation,
                mut image,
                ..
            } => match mode.as_str() {
                "invert" | "stale-response" | "malformed-output" | "wrong-dimensions" => {
                    if mode == "invert" {
                        invert(image.pixels_mut());
                    }
                    if mode == "wrong-dimensions" {
                        image = paint_plugin_bridge::protocol::BgraImage::new(
                            1,
                            1,
                            vec![0, 0, 0, 255],
                        )?;
                    }
                    if mode == "malformed-output" {
                        output.write_all(&[0_u8; 16])?;
                        output.flush()?;
                        return Ok(());
                    }
                    let response_generation = if mode == "stale-response" {
                        generation.saturating_sub(1)
                    } else {
                        generation
                    };
                    write_message(
                        &mut output,
                        &Message::ApplyResult {
                            request_id,
                            generation: response_generation,
                            image,
                        },
                    )?;
                }
                "hang" => thread::sleep(Duration::from_secs(60)),
                "crash" => std::process::exit(42),
                _ => {
                    write_message(
                        &mut output,
                        &Message::Error {
                            request_id,
                            generation,
                            code: RemoteErrorCode::InvalidRequest,
                            message: "unknown fixture mode".to_owned(),
                        },
                    )?;
                }
            },
            Message::Cancel { .. } => {}
            Message::Shutdown { request_id } => {
                write_message(&mut output, &Message::ShutdownAck { request_id })?;
                return Ok(());
            }
            other => {
                return Err(ProtocolError::InvalidPayload(format!(
                    "fixture received unexpected message {other:?}"
                )));
            }
        }
    }
}

fn invert(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[0] = 255 - pixel[0];
        pixel[1] = 255 - pixel[1];
        pixel[2] = 255 - pixel[2];
        // Alpha at byte 3 is deliberately unchanged.
    }
}
