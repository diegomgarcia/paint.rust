//! Data-only .NET Remoting Binary Format (MS-NRBF) records.
//!
//! Record layouts come from [MS-NRBF] version 13.0 (2024-11-19):
//! <https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nrbf/>.
//! PDN-specific field knowledge is documented separately in the PDN3 module.
//! This module never resolves a class name to a Rust type or executes a deserializer.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

mod parser;
mod writer;

pub type ObjectId = i32;

#[derive(Clone, Debug)]
pub struct NrbfLimits {
    pub max_stream_bytes: usize,
    pub max_output_bytes: usize,
    pub max_string_bytes: usize,
    pub max_depth: usize,
    pub max_members: usize,
    pub max_objects: usize,
    pub max_array_rank: usize,
    pub max_array_elements: usize,
    pub max_array_bytes: usize,
    pub max_records: usize,
}

impl Default for NrbfLimits {
    fn default() -> Self {
        Self {
            max_stream_bytes: 16 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
            max_string_bytes: 1024 * 1024,
            max_depth: 64,
            max_members: 4096,
            max_objects: 100_000,
            max_array_rank: 32,
            max_array_elements: 16 * 1024 * 1024,
            max_array_bytes: 64 * 1024 * 1024,
            max_records: 200_000,
        }
    }
}

#[derive(Debug, Error)]
pub enum NrbfError {
    #[error("truncated NRBF at byte {offset}: need {needed} bytes, only {remaining} remain")]
    UnexpectedEof {
        offset: usize,
        needed: usize,
        remaining: usize,
    },
    #[error("malformed NRBF at byte {offset}: {reason}")]
    Malformed { offset: usize, reason: &'static str },
    #[error("dangling NRBF object reference {id} at byte {offset}")]
    DanglingReference { offset: usize, id: ObjectId },
    #[error("unsupported NRBF record {record_type} at byte {offset}")]
    UnsupportedRecord { offset: usize, record_type: u8 },
    #[error("unsupported NRBF feature at byte {offset}: {feature}")]
    Unsupported {
        offset: usize,
        feature: &'static str,
    },
    #[error(
        "NRBF {limit} limit exceeded at byte {offset}: requested {requested}, maximum {maximum}"
    )]
    ResourceLimit {
        offset: usize,
        limit: &'static str,
        requested: u64,
        maximum: usize,
    },
    #[error("NRBF string at byte {offset} is not valid UTF-8")]
    InvalidUtf8 { offset: usize },
    #[error("could not allocate NRBF container for {elements} elements at byte {offset}")]
    Allocation { offset: usize, elements: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PrimitiveType {
    Boolean = 1,
    Byte = 2,
    Char = 3,
    Decimal = 5,
    Double = 6,
    Int16 = 7,
    Int32 = 8,
    Int64 = 9,
    SByte = 10,
    Single = 11,
    TimeSpan = 12,
    DateTime = 13,
    UInt16 = 14,
    UInt32 = 15,
    UInt64 = 16,
    Null = 17,
    String = 18,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveValue {
    Boolean(bool),
    Byte(u8),
    Char(char),
    Decimal(String),
    DoubleBits(u64),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    SByte(i8),
    SingleBits(u32),
    TimeSpan(i64),
    DateTime(u64),
    UInt16(u16),
    UInt32(u32),
    UInt64(u64),
    Null,
    String(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum BinaryType {
    Primitive(PrimitiveType),
    String,
    Object,
    SystemClass(String),
    Class { name: String, library_id: i32 },
    ObjectArray,
    StringArray,
    PrimitiveArray(PrimitiveType),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BinaryArrayType {
    Single = 0,
    Jagged = 1,
    Rectangular = 2,
    SingleOffset = 3,
    JaggedOffset = 4,
    RectangularOffset = 5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArrayEncoding {
    Binary(BinaryArrayType),
    SinglePrimitive,
    SingleObject,
    SingleString,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NrbfValue {
    Primitive(PrimitiveValue),
    Reference(ObjectId),
    Null,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NrbfClass {
    pub object_id: ObjectId,
    pub schema_id: ObjectId,
    pub members: Vec<NrbfValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassSchema {
    pub name: String,
    pub member_names: Vec<String>,
    pub member_types: Option<Vec<BinaryType>>,
    pub library_id: Option<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NrbfArray {
    pub encoding: ArrayEncoding,
    pub lengths: Vec<usize>,
    pub lower_bounds: Option<Vec<i32>>,
    pub element_type: BinaryType,
    pub items: Vec<NrbfValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NrbfObject {
    Class(NrbfClass),
    String(String),
    Array(NrbfArray),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NrbfStream {
    pub root_id: ObjectId,
    pub header_id: ObjectId,
    pub libraries: BTreeMap<i32, String>,
    pub schemas: BTreeMap<ObjectId, ClassSchema>,
    pub objects: BTreeMap<ObjectId, NrbfObject>,
}

impl NrbfStream {
    pub fn new(root_id: ObjectId, header_id: ObjectId) -> Self {
        Self {
            root_id,
            header_id,
            libraries: BTreeMap::new(),
            schemas: BTreeMap::new(),
            objects: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedNrbf {
    pub stream: NrbfStream,
    pub consumed: usize,
}

/// Parses one NRBF stream prefix. Bytes after MessageEnd are deliberately not consumed.
pub fn parse_nrbf_prefix(input: &[u8], limits: &NrbfLimits) -> Result<ParsedNrbf, NrbfError> {
    parser::parse(input, limits)
}

/// Serializes a data graph as NRBF without instantiating any file-named type.
pub fn write_nrbf(stream: &NrbfStream, limits: &NrbfLimits) -> Result<Vec<u8>, NrbfError> {
    writer::write(stream, limits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn i32_bytes(value: i32) -> [u8; 4] {
        value.to_le_bytes()
    }

    fn push_i32(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&i32_bytes(value));
    }

    fn push_string(output: &mut Vec<u8>, value: &str) {
        assert!(value.len() < 128);
        output.push(value.len() as u8);
        output.extend_from_slice(value.as_bytes());
    }

    fn header(root_id: i32) -> Vec<u8> {
        let mut bytes = vec![0];
        push_i32(&mut bytes, root_id);
        push_i32(&mut bytes, -1);
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, 0);
        bytes
    }

    /// A self-authored semantic NRBF fixture using only MS-NRBF record layouts.
    fn class_fixture() -> Vec<u8> {
        let mut bytes = header(1);
        bytes.push(12); // BinaryLibrary
        push_i32(&mut bytes, 2);
        push_string(&mut bytes, "Example.Library");
        bytes.push(5); // ClassWithMembersAndTypes
        push_i32(&mut bytes, 1);
        push_string(&mut bytes, "Example.Document");
        push_i32(&mut bytes, 3);
        push_string(&mut bytes, "width");
        push_string(&mut bytes, "title");
        push_string(&mut bytes, "items");
        bytes.extend_from_slice(&[0, 1, 5]); // Primitive, String, ObjectArray
        bytes.push(8); // Int32 additional info
        push_i32(&mut bytes, 2); // library ID
        push_i32(&mut bytes, 7); // width member, unrecorded primitive
        bytes.push(6); // BinaryObjectString
        push_i32(&mut bytes, 2);
        push_string(&mut bytes, "caf\u{00e9}");
        bytes.push(16); // ArraySingleObject
        push_i32(&mut bytes, 3);
        push_i32(&mut bytes, 2);
        bytes.push(15); // ArraySinglePrimitive
        push_i32(&mut bytes, 4);
        push_i32(&mut bytes, 3);
        bytes.push(2); // Byte
        bytes.extend_from_slice(&[9, 8, 7]);
        bytes.push(10); // ObjectNull, second object array item
        bytes.push(11); // MessageEnd
        bytes
    }

    fn byte_array_fixture(data: &[u8]) -> Vec<u8> {
        let mut bytes = header(1);
        bytes.push(15);
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, data.len() as i32);
        bytes.push(2);
        bytes.extend_from_slice(data);
        bytes.push(11);
        bytes
    }

    fn expect_byte_array(stream: &NrbfStream, id: ObjectId, expected: &[u8]) {
        let Some(NrbfObject::Array(array)) = stream.objects.get(&id) else {
            panic!("missing byte array")
        };
        let actual = array
            .items
            .iter()
            .map(|item| match item {
                NrbfValue::Primitive(PrimitiveValue::Byte(byte)) => *byte,
                other => panic!("unexpected array item: {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn parses_typed_class_strings_arrays_and_prefix_length() {
        let mut fixture = class_fixture();
        let stream_length = fixture.len();
        fixture.extend_from_slice(b"pixel chunks follow");

        let parsed = parse_nrbf_prefix(&fixture, &NrbfLimits::default()).expect("parse fixture");

        assert_eq!(parsed.consumed, stream_length);
        assert_eq!(parsed.stream.root_id, 1);
        assert_eq!(
            parsed.stream.libraries.get(&2).map(String::as_str),
            Some("Example.Library")
        );
        let Some(NrbfObject::Class(root)) = parsed.stream.objects.get(&1) else {
            panic!("root is not a class")
        };
        assert_eq!(
            root.members[0],
            NrbfValue::Primitive(PrimitiveValue::Int32(7))
        );
        assert_eq!(root.members[1], NrbfValue::Reference(2));
        assert_eq!(root.members[2], NrbfValue::Reference(3));
        assert_eq!(
            parsed.stream.objects.get(&2),
            Some(&NrbfObject::String("caf\u{00e9}".into()))
        );
        expect_byte_array(&parsed.stream, 4, &[9, 8, 7]);
    }

    #[test]
    fn every_strict_prefix_of_a_valid_stream_is_rejected_without_panicking() {
        let fixture = class_fixture();
        for end in 0..fixture.len() {
            assert!(
                parse_nrbf_prefix(&fixture[..end], &NrbfLimits::default()).is_err(),
                "prefix {end} unexpectedly parsed"
            );
        }
        parse_nrbf_prefix(&fixture, &NrbfLimits::default()).expect("complete fixture");
    }

    #[test]
    fn writer_round_trip_preserves_the_independent_fixture_graph() {
        let fixture = class_fixture();
        let first = parse_nrbf_prefix(&fixture, &NrbfLimits::default()).expect("parse fixture");
        let encoded = write_nrbf(&first.stream, &NrbfLimits::default()).expect("write graph");
        let second =
            parse_nrbf_prefix(&encoded, &NrbfLimits::default()).expect("parse written graph");
        assert_eq!(second.consumed, encoded.len());
        assert_eq!(second.stream, first.stream);
    }

    #[test]
    fn unknown_record_is_an_explicit_unsupported_error() {
        let mut fixture = header(1);
        fixture.push(99);
        assert!(matches!(
            parse_nrbf_prefix(&fixture, &NrbfLimits::default()),
            Err(NrbfError::UnsupportedRecord {
                record_type: 99,
                ..
            })
        ));
    }

    #[test]
    fn string_limit_is_checked_before_reading_or_allocating() {
        let mut fixture = header(1);
        fixture.push(6);
        push_i32(&mut fixture, 1);
        push_string(&mut fixture, "too long");
        fixture.push(11);
        let limits = NrbfLimits {
            max_string_bytes: 3,
            ..NrbfLimits::default()
        };
        assert!(matches!(
            parse_nrbf_prefix(&fixture, &limits),
            Err(NrbfError::ResourceLimit {
                limit: "string bytes",
                requested: 8,
                maximum: 3,
                ..
            })
        ));
    }

    #[test]
    fn huge_declared_array_is_rejected_before_allocation() {
        let mut fixture = header(1);
        fixture.push(15);
        push_i32(&mut fixture, 1);
        push_i32(&mut fixture, i32::MAX);
        fixture.push(2);
        assert!(matches!(
            parse_nrbf_prefix(&fixture, &NrbfLimits::default()),
            Err(NrbfError::ResourceLimit {
                limit: "array elements",
                ..
            })
        ));
    }

    #[test]
    fn excessive_nested_class_depth_is_rejected() {
        let mut fixture = header(1);
        fixture.push(4); // SystemClassWithMembersAndTypes
        push_i32(&mut fixture, 1);
        push_string(&mut fixture, "Node");
        push_i32(&mut fixture, 1);
        push_string(&mut fixture, "child");
        fixture.push(2); // Object
        fixture.push(1); // ClassWithId, nested object
        push_i32(&mut fixture, 2);
        push_i32(&mut fixture, 1);
        fixture.push(10); // nested child = null
        fixture.push(10); // root child = null is not present: nested object occupies it
        fixture.push(11);
        let limits = NrbfLimits {
            max_depth: 1,
            ..NrbfLimits::default()
        };
        assert!(matches!(
            parse_nrbf_prefix(&fixture, &limits),
            Err(NrbfError::ResourceLimit {
                limit: "object depth",
                ..
            })
        ));
    }

    #[test]
    fn dangling_reference_and_duplicate_object_ids_are_rejected() {
        let mut dangling = header(1);
        dangling.push(4);
        push_i32(&mut dangling, 1);
        push_string(&mut dangling, "Node");
        push_i32(&mut dangling, 1);
        push_string(&mut dangling, "child");
        dangling.push(2);
        dangling.push(9);
        push_i32(&mut dangling, 99);
        dangling.push(11);
        assert!(matches!(
            parse_nrbf_prefix(&dangling, &NrbfLimits::default()),
            Err(NrbfError::DanglingReference { id: 99, .. })
        ));

        let mut duplicate = byte_array_fixture(b"a");
        duplicate.pop();
        duplicate.push(6);
        push_i32(&mut duplicate, 1);
        push_string(&mut duplicate, "duplicate");
        duplicate.push(11);
        assert!(matches!(
            parse_nrbf_prefix(&duplicate, &NrbfLimits::default()),
            Err(NrbfError::Malformed {
                reason: "duplicate object id",
                ..
            })
        ));
    }

    #[test]
    fn null_run_cannot_overshoot_array_bounds() {
        let mut fixture = header(1);
        fixture.push(16);
        push_i32(&mut fixture, 1);
        push_i32(&mut fixture, 1);
        fixture.push(13);
        fixture.push(2);
        fixture.push(11);
        assert!(matches!(
            parse_nrbf_prefix(&fixture, &NrbfLimits::default()),
            Err(NrbfError::Malformed {
                reason: "null run exceeds member count",
                ..
            })
        ));
    }

    #[test]
    fn negative_inline_object_id_is_valid_but_negative_member_reference_is_not() {
        let mut fixture = header(1);
        fixture.push(4);
        push_i32(&mut fixture, 1);
        push_string(&mut fixture, "Root");
        push_i32(&mut fixture, 1);
        push_string(&mut fixture, "value");
        fixture.push(1); // String
        let inline_record_offset = fixture.len();
        fixture.push(6); // inline BinaryObjectString
        push_i32(&mut fixture, -8);
        push_string(&mut fixture, "inline");
        fixture.push(11);
        let parsed = parse_nrbf_prefix(&fixture, &NrbfLimits::default())
            .expect("negative inline id is permitted by MS-NRBF");
        let encoded = write_nrbf(&parsed.stream, &NrbfLimits::default()).expect("write inline id");
        let reparsed =
            parse_nrbf_prefix(&encoded, &NrbfLimits::default()).expect("reparse inline id");
        assert_eq!(reparsed.stream, parsed.stream);

        fixture[inline_record_offset] = 9; // MemberReference instead of inline string
        assert!(matches!(
            parse_nrbf_prefix(&fixture, &NrbfLimits::default()),
            Err(NrbfError::Malformed {
                reason: "object reference id must be positive",
                ..
            })
        ));
    }

    #[test]
    fn self_cycle_is_data_and_writer_terminates() {
        let mut fixture = header(1);
        fixture.push(4);
        push_i32(&mut fixture, 1);
        push_string(&mut fixture, "Node");
        push_i32(&mut fixture, 1);
        push_string(&mut fixture, "child");
        fixture.push(2);
        fixture.push(9);
        push_i32(&mut fixture, 1);
        fixture.push(11);
        let parsed = parse_nrbf_prefix(&fixture, &NrbfLimits::default()).expect("parse cycle");
        let encoded = write_nrbf(&parsed.stream, &NrbfLimits::default()).expect("write cycle");
        let reparsed = parse_nrbf_prefix(&encoded, &NrbfLimits::default()).expect("reparse cycle");
        assert_eq!(reparsed.stream, parsed.stream);
    }

    #[test]
    fn rectangular_offset_array_round_trips_dimensions_bounds_and_values() {
        let mut fixture = header(1);
        fixture.push(7); // BinaryArray
        push_i32(&mut fixture, 1);
        fixture.push(5); // RectangularOffset
        push_i32(&mut fixture, 2); // rank
        push_i32(&mut fixture, 1);
        push_i32(&mut fixture, 2);
        push_i32(&mut fixture, -1);
        push_i32(&mut fixture, 3);
        fixture.push(0); // Primitive
        fixture.push(8); // Int32
        push_i32(&mut fixture, -42);
        push_i32(&mut fixture, 1_000_000);
        fixture.push(11);

        let parsed = parse_nrbf_prefix(&fixture, &NrbfLimits::default()).expect("parse array");
        let Some(NrbfObject::Array(array)) = parsed.stream.objects.get(&1) else {
            panic!("missing array")
        };
        assert_eq!(array.lengths, vec![1, 2]);
        assert_eq!(array.lower_bounds, Some(vec![-1, 3]));
        assert_eq!(
            array.items,
            vec![
                NrbfValue::Primitive(PrimitiveValue::Int32(-42)),
                NrbfValue::Primitive(PrimitiveValue::Int32(1_000_000)),
            ]
        );
        let encoded = write_nrbf(&parsed.stream, &NrbfLimits::default()).expect("write array");
        let reparsed = parse_nrbf_prefix(&encoded, &NrbfLimits::default()).expect("reparse array");
        assert_eq!(reparsed.stream, parsed.stream);
    }

    #[test]
    fn single_primitive_arrays_reject_string_and_null_element_types() {
        for primitive in [17, 18] {
            let mut fixture = header(1);
            fixture.push(15);
            push_i32(&mut fixture, 1);
            push_i32(&mut fixture, 0);
            fixture.push(primitive);
            fixture.push(11);
            assert!(matches!(
                parse_nrbf_prefix(&fixture, &NrbfLimits::default()),
                Err(NrbfError::Malformed {
                    reason: "invalid single primitive array element type",
                    ..
                })
            ));
        }
    }

    #[test]
    fn nonminimal_and_overflowing_string_lengths_are_rejected() {
        for prefix in [&[0x80, 0x00][..], &[0xff, 0xff, 0xff, 0xff, 0x08][..]] {
            let mut fixture = header(1);
            fixture.push(6);
            push_i32(&mut fixture, 1);
            fixture.extend_from_slice(prefix);
            assert!(parse_nrbf_prefix(&fixture, &NrbfLimits::default()).is_err());
        }
    }

    #[test]
    fn writer_enforces_output_and_string_limits() {
        let parsed = parse_nrbf_prefix(&class_fixture(), &NrbfLimits::default()).expect("fixture");
        let limits = NrbfLimits {
            max_output_bytes: 16,
            ..NrbfLimits::default()
        };
        assert!(matches!(
            write_nrbf(&parsed.stream, &limits),
            Err(NrbfError::ResourceLimit {
                limit: "NRBF output bytes",
                ..
            })
        ));
        let limits = NrbfLimits {
            max_string_bytes: 2,
            ..NrbfLimits::default()
        };
        assert!(matches!(
            write_nrbf(&parsed.stream, &limits),
            Err(NrbfError::ResourceLimit {
                limit: "string bytes",
                ..
            })
        ));
    }

    proptest! {
        #[test]
        fn arbitrary_byte_arrays_survive_independent_parse_and_write(
            data in prop::collection::vec(any::<u8>(), 0..512),
            trailing in prop::collection::vec(any::<u8>(), 0..16),
        ) {
            let fixture = byte_array_fixture(&data);
            let stream_length = fixture.len();
            let mut with_trailing = fixture;
            with_trailing.extend_from_slice(&trailing);
            let parsed = parse_nrbf_prefix(&with_trailing, &NrbfLimits::default())
                .expect("valid independent byte-array fixture");
            prop_assert_eq!(parsed.consumed, stream_length);
            expect_byte_array(&parsed.stream, 1, &data);
            let encoded = write_nrbf(&parsed.stream, &NrbfLimits::default())
                .expect("serialize parsed array");
            let reparsed = parse_nrbf_prefix(&encoded, &NrbfLimits::default())
                .expect("parse serialized array");
            prop_assert_eq!(reparsed.stream, parsed.stream);
        }

        #[test]
        fn malformed_inputs_never_consume_past_the_input(
            suffix in prop::collection::vec(any::<u8>(), 0..128)
        ) {
            let mut input = header(1);
            input.extend_from_slice(&suffix);
            if let Ok(parsed) = parse_nrbf_prefix(&input, &NrbfLimits::default()) {
                prop_assert!(parsed.consumed <= input.len());
            }
        }
    }
}
