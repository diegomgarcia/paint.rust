use std::io::Cursor;

use paint_plugin_bridge::protocol::{
    BgraImage, Frame, HEADER_LEN, MAGIC, MAX_ERROR_MESSAGE_BYTES, MAX_PAYLOAD_LEN, Message,
    MessageKind, PROTOCOL_VERSION, ProtocolError, RemoteErrorCode, read_frame, read_message,
    write_frame, write_message,
};

fn header(version: u16, kind: u8, flags: u8, payload_len: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_LEN);
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.push(kind);
    bytes.push(flags);
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes.resize(HEADER_LEN, 0);
    bytes
}

#[test]
fn apply_round_trip_preserves_bgra_request_and_generation() {
    let pixels = vec![1, 2, 3, 4, 250, 251, 252, 0];
    let message = Message::Apply {
        request_id: 41,
        generation: 9,
        effect: "Invert".to_owned(),
        image: BgraImage::new(2, 1, pixels.clone()).unwrap(),
    };

    let frame = message.encode().unwrap();
    assert_eq!(frame.kind, MessageKind::Apply);
    let pixel_offset = 26 + "Invert".len();
    assert_eq!(frame.payload.len(), pixel_offset + pixels.len());
    assert_eq!(&frame.payload[pixel_offset..], pixels);
    assert_eq!(Message::decode(frame).unwrap(), message);

    let mut wire = Vec::new();
    write_message(&mut wire, &message).unwrap();
    assert_eq!(read_message(&mut Cursor::new(wire)).unwrap(), message);
}

#[test]
fn frame_round_trip_uses_versioned_binary_header() {
    let frame = Frame::new(MessageKind::Cancel, 17_u64.to_le_bytes().to_vec()).unwrap();
    let mut wire = Vec::new();
    write_frame(&mut wire, &frame).unwrap();

    assert_eq!(&wire[..4], &MAGIC);
    assert_eq!(u16::from_le_bytes([wire[4], wire[5]]), PROTOCOL_VERSION);
    assert_eq!(wire[6], MessageKind::Cancel as u8);
    assert_eq!(wire[7], 0);
    assert_eq!(read_frame(&mut Cursor::new(wire)).unwrap(), frame);
}

#[test]
fn malformed_headers_are_rejected() {
    let mut bad_magic = header(PROTOCOL_VERSION, MessageKind::Hello as u8, 0, 0);
    bad_magic[0] ^= 0xff;
    assert!(matches!(
        read_frame(&mut Cursor::new(bad_magic)),
        Err(ProtocolError::BadMagic(_))
    ));

    for version in [0, PROTOCOL_VERSION + 1] {
        let bytes = header(version, MessageKind::Hello as u8, 0, 0);
        assert!(matches!(
            read_frame(&mut Cursor::new(bytes)),
            Err(ProtocolError::UnsupportedVersion(value)) if value == version
        ));
    }

    let bytes = header(PROTOCOL_VERSION, MessageKind::Hello as u8, 1, 0);
    assert!(matches!(
        read_frame(&mut Cursor::new(bytes)),
        Err(ProtocolError::InvalidFlags(1))
    ));

    let bytes = header(PROTOCOL_VERSION, 0xff, 0, 0);
    assert!(matches!(
        read_frame(&mut Cursor::new(bytes)),
        Err(ProtocolError::UnknownMessageKind(0xff))
    ));
}

#[test]
fn oversized_frame_is_rejected_before_reading_a_payload() {
    let bytes = header(
        PROTOCOL_VERSION,
        MessageKind::Apply as u8,
        0,
        (MAX_PAYLOAD_LEN + 1) as u32,
    );
    assert!(matches!(
        read_frame(&mut Cursor::new(bytes)),
        Err(ProtocolError::PayloadTooLarge { .. })
    ));
}

#[test]
fn truncated_frames_are_not_accepted() {
    let short_header = vec![0_u8; HEADER_LEN - 1];
    assert!(matches!(
        read_frame(&mut Cursor::new(short_header)),
        Err(ProtocolError::Truncated)
    ));

    let mut short_payload = header(PROTOCOL_VERSION, MessageKind::Hello as u8, 0, 8);
    short_payload.extend_from_slice(&[0_u8; 3]);
    assert!(matches!(
        read_frame(&mut Cursor::new(short_payload)),
        Err(ProtocolError::Truncated)
    ));
}

#[test]
fn malformed_apply_payloads_are_rejected() {
    let valid = BgraImage::new(1, 1, vec![1, 2, 3, 4]).unwrap();
    let message = Message::Apply {
        request_id: 1,
        generation: 3,
        effect: "Invert".to_owned(),
        image: valid,
    };
    let mut frame = message.encode().unwrap();
    frame.payload.pop();
    assert!(matches!(
        Message::decode(frame),
        Err(ProtocolError::InvalidPayload(_))
    ));

    let zero_dimension = BgraImage::new(0, 1, Vec::new());
    assert!(matches!(
        zero_dimension,
        Err(ProtocolError::InvalidImage(_))
    ));

    let wrong_length = BgraImage::new(2, 2, vec![0; 15]);
    assert!(matches!(wrong_length, Err(ProtocolError::InvalidImage(_))));
}

#[test]
fn invalid_utf8_and_unbounded_metadata_are_rejected() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&1_u16.to_le_bytes());
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.push(0xff);
    payload.extend_from_slice(&[1, 2, 3, 4]);
    let frame = Frame::new(MessageKind::Apply, payload).unwrap();
    assert!(matches!(
        Message::decode(frame),
        Err(ProtocolError::InvalidPayload(_))
    ));

    let message = Message::Error {
        request_id: 1,
        generation: 1,
        code: RemoteErrorCode::Internal,
        message: "x".repeat(MAX_ERROR_MESSAGE_BYTES + 1),
    };
    assert!(matches!(
        message.encode(),
        Err(ProtocolError::InvalidPayload(_))
    ));
}

#[test]
fn zero_request_ids_are_invalid() {
    let message = Message::Hello { request_id: 0 };
    assert!(matches!(
        message.encode(),
        Err(ProtocolError::InvalidPayload(_))
    ));
}
