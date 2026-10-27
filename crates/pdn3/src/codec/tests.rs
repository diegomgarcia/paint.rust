use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use paint_core::{Bgra, Size, Surface};
use paint_document::{Layer, LayerId};
use proptest::prelude::*;

use super::*;
use crate::nrbf::*;
use crate::{PdnChunkCompression, PdnReport, PdnUnsupported};

const PIXELS: [u8; 16] = [3, 2, 1, 4, 7, 128, 250, 0, 9, 8, 7, 255, 0, 0, 0, 0];

fn primitive(value: PrimitiveValue) -> NrbfValue {
    NrbfValue::Primitive(value)
}

fn int32(value: i32) -> NrbfValue {
    primitive(PrimitiveValue::Int32(value))
}

fn boolean(value: bool) -> NrbfValue {
    primitive(PrimitiveValue::Boolean(value))
}

fn reference(id: ObjectId) -> NrbfValue {
    NrbfValue::Reference(id)
}

fn class_type(name: &str) -> BinaryType {
    BinaryType::Class {
        name: name.into(),
        library_id: 2,
    }
}

fn system_class(name: &str) -> BinaryType {
    BinaryType::SystemClass(name.into())
}

fn add_class(
    stream: &mut NrbfStream,
    id: ObjectId,
    name: &str,
    library_id: Option<i32>,
    members: Vec<(&str, BinaryType, NrbfValue)>,
) {
    let (member_names, member_types, values) = members.into_iter().fold(
        (Vec::new(), Vec::new(), Vec::new()),
        |(mut names, mut types, mut values), (name, binary_type, value)| {
            names.push(name.to_owned());
            types.push(binary_type);
            values.push(value);
            (names, types, values)
        },
    );
    stream.schemas.insert(
        id,
        ClassSchema {
            name: name.into(),
            member_names,
            member_types: Some(member_types),
            library_id,
        },
    );
    stream.objects.insert(
        id,
        NrbfObject::Class(NrbfClass {
            object_id: id,
            schema_id: id,
            members: values,
        }),
    );
}

fn add_instance(stream: &mut NrbfStream, id: ObjectId, schema: ObjectId, values: Vec<NrbfValue>) {
    stream.objects.insert(
        id,
        NrbfObject::Class(NrbfClass {
            object_id: id,
            schema_id: schema,
            members: values,
        }),
    );
}

fn add_string(stream: &mut NrbfStream, id: ObjectId, value: &str) {
    stream.objects.insert(id, NrbfObject::String(value.into()));
}

fn add_array(
    stream: &mut NrbfStream,
    id: ObjectId,
    encoding: ArrayEncoding,
    element_type: BinaryType,
    items: Vec<NrbfValue>,
) {
    let length = items.len();
    stream.objects.insert(
        id,
        NrbfObject::Array(NrbfArray {
            encoding,
            lengths: vec![length],
            lower_bounds: None,
            element_type,
            items,
        }),
    );
}

const METADATA_ENTRY: &str = "System.Collections.Generic.KeyValuePair`2[[System.String, mscorlib],[System.String, mscorlib]]";

fn metadata_type() -> BinaryType {
    system_class(
        "System.Collections.Generic.KeyValuePair`2[[System.String, mscorlib],[System.String, mscorlib]][]",
    )
}

struct FixtureConfig {
    gzip: bool,
    shared_layers: bool,
    blend_mode: i32,
    stride: i32,
    metadata: bool,
}

impl Default for FixtureConfig {
    fn default() -> Self {
        Self {
            gzip: false,
            shared_layers: false,
            blend_mode: 0,
            stride: 12,
            metadata: false,
        }
    }
}

struct Fixture {
    bytes: Vec<u8>,
    chunks_offset: usize,
}

fn semantic_fixture(config: &FixtureConfig) -> Fixture {
    let mut stream = NrbfStream::new(1, -1);
    stream.libraries.insert(2, "PaintDotNet.Data".into());
    stream.libraries.insert(3, "PaintDotNet.Core".into());
    let layer_count = if config.shared_layers { 2 } else { 1 };
    add_class(
        &mut stream,
        1,
        "PaintDotNet.Document",
        Some(2),
        vec![
            (
                "isDisposed",
                BinaryType::Primitive(PrimitiveType::Boolean),
                boolean(false),
            ),
            ("layers", class_type("PaintDotNet.LayerList"), reference(3)),
            (
                "width",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(2),
            ),
            (
                "height",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(2),
            ),
            ("savedWith", system_class("System.Version"), reference(4)),
            ("userMetadataItems", metadata_type(), reference(5)),
            (
                "futureDocumentField",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(99),
            ),
        ],
    );
    add_class(
        &mut stream,
        3,
        "PaintDotNet.LayerList",
        Some(2),
        vec![
            ("parent", class_type("PaintDotNet.Document"), reference(1)),
            ("ArrayList+_items", BinaryType::ObjectArray, reference(7)),
            (
                "ArrayList+_size",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(layer_count),
            ),
            (
                "ArrayList+_version",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(layer_count),
            ),
        ],
    );
    add_class(
        &mut stream,
        4,
        "System.Version",
        None,
        vec![
            (
                "_Major",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(4),
            ),
            (
                "_Minor",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(21),
            ),
            (
                "_Build",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(6589),
            ),
            (
                "_Revision",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(7045),
            ),
        ],
    );
    add_array(
        &mut stream,
        5,
        ArrayEncoding::Binary(BinaryArrayType::Single),
        metadata_type(),
        if config.metadata {
            vec![reference(60)]
        } else {
            vec![]
        },
    );
    add_array(
        &mut stream,
        7,
        ArrayEncoding::SingleObject,
        BinaryType::Object,
        if config.shared_layers {
            vec![reference(20), reference(40)]
        } else {
            vec![reference(20)]
        },
    );
    let blend_name = if config.blend_mode == 0 {
        "PaintDotNet.UserBlendOps+NormalBlendOp"
    } else {
        "PaintDotNet.UserBlendOps+AdditiveBlendOp"
    };
    add_class(
        &mut stream,
        20,
        "PaintDotNet.BitmapLayer",
        Some(2),
        vec![
            (
                "properties",
                class_type("PaintDotNet.BitmapLayer+BitmapLayerProperties"),
                reference(23),
            ),
            (
                "surface",
                BinaryType::Class {
                    name: "PaintDotNet.Surface".into(),
                    library_id: 3,
                },
                reference(24),
            ),
            (
                "Layer+isDisposed",
                BinaryType::Primitive(PrimitiveType::Boolean),
                boolean(false),
            ),
            (
                "Layer+width",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(2),
            ),
            (
                "Layer+height",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(2),
            ),
            (
                "Layer+properties",
                class_type("PaintDotNet.Layer+LayerProperties"),
                reference(25),
            ),
        ],
    );
    add_class(
        &mut stream,
        23,
        "PaintDotNet.BitmapLayer+BitmapLayerProperties",
        Some(2),
        vec![("blendOp", class_type(blend_name), reference(29))],
    );
    add_class(
        &mut stream,
        24,
        "PaintDotNet.Surface",
        Some(3),
        vec![
            (
                "width",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(2),
            ),
            (
                "height",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(2),
            ),
            (
                "stride",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(config.stride),
            ),
            (
                "scan0",
                BinaryType::Class {
                    name: "PaintDotNet.MemoryBlock".into(),
                    library_id: 3,
                },
                reference(30),
            ),
        ],
    );
    add_class(
        &mut stream,
        25,
        "PaintDotNet.Layer+LayerProperties",
        Some(2),
        vec![
            ("name", BinaryType::String, reference(31)),
            ("userMetadataItems", metadata_type(), reference(32)),
            (
                "visible",
                BinaryType::Primitive(PrimitiveType::Boolean),
                boolean(false),
            ),
            (
                "isBackground",
                BinaryType::Primitive(PrimitiveType::Boolean),
                boolean(true),
            ),
            (
                "opacity",
                BinaryType::Primitive(PrimitiveType::Byte),
                primitive(PrimitiveValue::Byte(37)),
            ),
            (
                "blendMode",
                class_type("PaintDotNet.LayerBlendMode"),
                reference(-33),
            ),
        ],
    );
    add_class(&mut stream, 29, blend_name, Some(2), vec![]);
    add_class(
        &mut stream,
        30,
        "PaintDotNet.MemoryBlock",
        Some(3),
        vec![
            (
                "length64",
                BinaryType::Primitive(PrimitiveType::Int64),
                primitive(PrimitiveValue::Int64(i64::from(config.stride) * 2)),
            ),
            (
                "hasParent",
                BinaryType::Primitive(PrimitiveType::Boolean),
                boolean(false),
            ),
            (
                "deferred",
                BinaryType::Primitive(PrimitiveType::Boolean),
                boolean(true),
            ),
        ],
    );
    add_string(&mut stream, 31, "Layer <&> ☃");
    add_array(
        &mut stream,
        32,
        ArrayEncoding::Binary(BinaryArrayType::Single),
        metadata_type(),
        if config.metadata {
            vec![reference(63)]
        } else {
            vec![]
        },
    );
    add_class(
        &mut stream,
        -33,
        "PaintDotNet.LayerBlendMode",
        Some(2),
        vec![(
            "value__",
            BinaryType::Primitive(PrimitiveType::Int32),
            int32(config.blend_mode),
        )],
    );
    if config.shared_layers {
        add_instance(
            &mut stream,
            40,
            20,
            vec![
                reference(23),
                reference(24),
                boolean(false),
                int32(2),
                int32(2),
                reference(25),
            ],
        );
    }
    if config.metadata {
        add_class(
            &mut stream,
            60,
            METADATA_ENTRY,
            None,
            vec![
                ("key", BinaryType::String, reference(61)),
                ("value", BinaryType::String, reference(62)),
            ],
        );
        add_string(&mut stream, 61, "DocumentKey");
        add_string(&mut stream, 62, "DocumentValue");
        add_instance(&mut stream, 63, 60, vec![reference(64), reference(65)]);
        add_string(&mut stream, 64, "LayerKey");
        add_string(&mut stream, 65, "LayerValue");
    }
    let nrbf = write_nrbf(&stream, &NrbfLimits::default()).expect("write independent graph");

    let mut raw = Vec::new();
    for row in 0..2 {
        raw.extend_from_slice(&PIXELS[row * 8..row * 8 + 8]);
        raw.extend_from_slice(&[99, 98, 97, 96][..(config.stride as usize - 8)]);
    }
    let mut chunks = Vec::new();
    chunks.push(u8::from(!config.gzip));
    chunks.extend_from_slice(&5u32.to_be_bytes());
    for (index, chunk) in raw.chunks(5).enumerate() {
        chunks.extend_from_slice(&(index as u32).to_be_bytes());
        let encoded = if config.gzip {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(chunk).expect("gzip fixture");
            encoder.finish().expect("gzip fixture finish")
        } else {
            chunk.to_vec()
        };
        chunks.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
        chunks.extend_from_slice(&encoded);
    }

    let xml = format!(
        "<pdnImage width=\"2\" height=\"2\" layers=\"{layer_count}\" savedWithVersion=\"4.21.6589.7045\"><custom><future value=\"&quot;kept&quot;\" /></custom></pdnImage>"
    );
    let mut bytes = b"PDN3".to_vec();
    bytes.extend_from_slice(&(xml.len() as u32).to_le_bytes()[..3]);
    bytes.extend_from_slice(xml.as_bytes());
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&nrbf);
    let chunks_offset = bytes.len();
    bytes.extend_from_slice(&chunks);
    Fixture {
        bytes,
        chunks_offset,
    }
}

fn out_of_order_raw_fixture() -> Vec<u8> {
    let fixture = semantic_fixture(&FixtureConfig::default());
    let mut records = Vec::new();
    let mut position = fixture.chunks_offset + 5;
    while position < fixture.bytes.len() {
        let stored_size = u32::from_be_bytes(
            fixture.bytes[position + 4..position + 8]
                .try_into()
                .expect("chunk size"),
        ) as usize;
        let end = position + 8 + stored_size;
        records.push(fixture.bytes[position..end].to_vec());
        position = end;
    }
    assert_eq!(records.len(), 5);
    let mut bytes = fixture.bytes[..fixture.chunks_offset + 5].to_vec();
    for index in [2, 0, 4, 1, 3] {
        bytes.extend_from_slice(&records[index]);
    }
    bytes
}

fn expected_document() -> Document {
    let pixels = PIXELS
        .chunks_exact(4)
        .map(|bytes| Bgra::from_bgra_bytes(bytes.try_into().expect("pixel")))
        .collect::<Vec<_>>();
    let surface = Surface::from_pixels(Size::new(2, 2).expect("size"), pixels).expect("surface");
    let layer = Layer::new(LayerId::new(1), "Layer <&> ☃", surface)
        .with_visibility(false)
        .with_opacity(37)
        .with_background(true);
    Document::from_layers(Size::new(2, 2).expect("size"), vec![layer]).expect("document")
}

#[test]
fn independent_raw_and_gzip_fixtures_decode_exact_semantics() {
    for gzip in [false, true] {
        let fixture = semantic_fixture(&FixtureConfig {
            gzip,
            ..FixtureConfig::default()
        });
        let decoded = decode_pdn(&fixture.bytes).expect("decode semantic fixture");
        assert_eq!(decoded.document, expected_document());
        assert_eq!(decoded.producer_version, "4.21.6589.7045");
        assert!(decoded.header_xml.contains("future value="));
        assert_eq!(decoded.report.padded_layers, vec![0]);
        assert_eq!(decoded.report.unmapped_members.len(), 1);
        assert_eq!(
            decoded.report.unmapped_members[0].member_name,
            "futureDocumentField"
        );
        assert!(decoded.trailing_bytes.is_empty());
    }
}

#[test]
fn metadata_is_reported_and_original_objects_are_preserved() {
    let fixture = semantic_fixture(&FixtureConfig {
        metadata: true,
        ..FixtureConfig::default()
    });
    let decoded = decode_pdn(&fixture.bytes).expect("decode metadata fixture");
    assert_eq!(decoded.report.document_metadata_entries, 1);
    assert_eq!(decoded.report.layer_metadata_entries, [1]);
    assert!(decoded.report.unmapped_objects.is_empty());
    assert!(decoded.nrbf.objects.contains_key(&60));
    assert!(decoded.nrbf.objects.contains_key(&63));
    assert_eq!(decoded.document, expected_document());
}

#[test]
fn normal_writer_emits_real_container_and_raw_or_gzip_chunks() {
    for compression in [PdnChunkCompression::Raw, PdnChunkCompression::Gzip] {
        let options = PdnWriteOptions {
            compression,
            chunk_size: 5,
        };
        let encoded = encode_pdn_with_options(&expected_document(), &options).expect("encode PDN");
        assert_eq!(&encoded[..4], b"PDN3");
        let decoded = decode_pdn(&encoded).expect("decode written PDN");
        assert_eq!(decoded.document, expected_document());
        assert_eq!(decoded.producer_version, "0.1.0.0");
        assert_eq!(
            decoded.report,
            PdnReport {
                layer_metadata_entries: vec![0],
                ..PdnReport::default()
            }
        );
    }
}

#[test]
fn writer_enforces_output_and_chunk_limits() {
    let document = expected_document();
    let options = PdnWriteOptions {
        compression: PdnChunkCompression::Gzip,
        chunk_size: 5,
    };
    let output = PdnLimits {
        max_output_bytes: 64,
        ..PdnLimits::default()
    };
    assert!(matches!(
        encode_pdn_with_options_and_limits(&document, &options, &output),
        Err(PdnError::ResourceLimit {
            resource: "output bytes",
            ..
        })
    ));
    let compressed = PdnLimits {
        max_compressed_chunk_bytes: 1,
        ..PdnLimits::default()
    };
    assert!(matches!(
        encode_pdn_with_options_and_limits(&document, &options, &compressed),
        Err(PdnError::ResourceLimit {
            resource: "compressed chunk bytes",
            ..
        })
    ));
    let chunks = PdnLimits {
        max_chunks_per_surface: 3,
        ..PdnLimits::default()
    };
    assert!(matches!(
        encode_pdn_with_options_and_limits(&document, &options, &chunks),
        Err(PdnError::ResourceLimit {
            resource: "chunks per surface",
            ..
        })
    ));
}

#[test]
fn non_normal_blend_is_explicitly_unsupported() {
    let fixture = semantic_fixture(&FixtureConfig {
        blend_mode: 2,
        ..FixtureConfig::default()
    });
    assert!(matches!(
        decode_pdn(&fixture.bytes),
        Err(PdnError::Unsupported(PdnUnsupported::BlendMode(2)))
    ));
}

#[test]
fn shared_pixel_memory_is_reused_without_a_second_chunk_stream() {
    let fixture = semantic_fixture(&FixtureConfig {
        shared_layers: true,
        ..FixtureConfig::default()
    });
    let decoded = decode_pdn(&fixture.bytes).expect("decode shared layers");
    assert_eq!(decoded.document.layers().len(), 2);
    assert_eq!(
        decoded.document.layers()[0].surface(),
        decoded.document.layers()[1].surface()
    );
    assert_eq!(decoded.report.shared_layers, vec![1]);
}

#[test]
fn trailing_data_is_preserved_and_reported() {
    let mut fixture = semantic_fixture(&FixtureConfig::default());
    fixture.bytes.extend_from_slice(&[9, 8, 7]);
    let decoded = decode_pdn(&fixture.bytes).expect("decode with trailing data");
    assert_eq!(decoded.trailing_bytes, [9, 8, 7]);
    assert_eq!(decoded.report.trailing_bytes, 3);
}

#[test]
fn out_of_order_chunks_restore_exact_pixel_offsets() {
    let decoded = decode_pdn(&out_of_order_raw_fixture()).expect("decode reordered chunks");
    assert_eq!(decoded.document, expected_document());
}

#[test]
fn invalid_headers_xml_and_every_truncated_prefix_fail() {
    let fixture = semantic_fixture(&FixtureConfig::default());
    for end in 0..fixture.bytes.len() {
        assert!(
            decode_pdn(&fixture.bytes[..end]).is_err(),
            "prefix {end} parsed"
        );
    }
    let mut bad_magic = fixture.bytes.clone();
    bad_magic[3] = b'4';
    assert!(matches!(
        decode_pdn(&bad_magic),
        Err(PdnError::InvalidMagic)
    ));
    let mut bad_indicator = fixture.bytes.clone();
    let indicator = bad_indicator
        .windows(2)
        .position(|window| window == [0, 1])
        .expect("indicator");
    bad_indicator[indicator + 1] = 2;
    assert!(decode_pdn(&bad_indicator).is_err());
    let mut bad_xml = fixture.bytes.clone();
    bad_xml[7] = b'!';
    assert!(decode_pdn(&bad_xml).is_err());
}

#[test]
fn malformed_chunk_numbers_sizes_compression_and_versions_are_rejected() {
    let base = semantic_fixture(&FixtureConfig::default());
    let mut out_of_range = base.bytes.clone();
    out_of_range[base.chunks_offset + 5..base.chunks_offset + 9]
        .copy_from_slice(&99u32.to_be_bytes());
    assert!(decode_pdn(&out_of_range).is_err());

    let mut duplicate = base.bytes.clone();
    duplicate[base.chunks_offset + 23..base.chunks_offset + 27]
        .copy_from_slice(&0u32.to_be_bytes());
    assert!(decode_pdn(&duplicate).is_err());

    let mut wrong_raw_size = base.bytes.clone();
    wrong_raw_size[base.chunks_offset + 9..base.chunks_offset + 13]
        .copy_from_slice(&4u32.to_be_bytes());
    assert!(decode_pdn(&wrong_raw_size).is_err());

    let mut unknown_version = base.bytes.clone();
    unknown_version[base.chunks_offset] = 9;
    assert!(matches!(
        decode_pdn(&unknown_version),
        Err(PdnError::Unsupported(PdnUnsupported::ChunkFormat(9)))
    ));

    let gzip = semantic_fixture(&FixtureConfig {
        gzip: true,
        ..FixtureConfig::default()
    });
    let mut corrupt = gzip.bytes.clone();
    corrupt[gzip.chunks_offset + 13] ^= 0xff;
    assert!(decode_pdn(&corrupt).is_err());
}

#[test]
fn container_semantic_and_chunk_resource_limits_are_typed() {
    let fixture = semantic_fixture(&FixtureConfig::default());
    let cases = [
        (
            PdnLimits {
                max_input_bytes: fixture.bytes.len() - 1,
                ..PdnLimits::default()
            },
            "input bytes",
        ),
        (
            PdnLimits {
                max_header_bytes: 8,
                ..PdnLimits::default()
            },
            "header bytes",
        ),
        (
            PdnLimits {
                max_layers: 0,
                ..PdnLimits::default()
            },
            "layers",
        ),
        (
            PdnLimits {
                max_pixels: 3,
                ..PdnLimits::default()
            },
            "pixels",
        ),
        (
            PdnLimits {
                max_surface_bytes: 23,
                ..PdnLimits::default()
            },
            "surface bytes",
        ),
        (
            PdnLimits {
                max_total_surface_bytes: 15,
                ..PdnLimits::default()
            },
            "total surface bytes",
        ),
        (
            PdnLimits {
                max_chunk_bytes: 4,
                ..PdnLimits::default()
            },
            "chunk bytes",
        ),
        (
            PdnLimits {
                max_chunks_per_surface: 4,
                ..PdnLimits::default()
            },
            "chunks per surface",
        ),
    ];
    for (limits, resource) in cases {
        assert!(
            matches!(
                decode_pdn_with_limits(&fixture.bytes, &limits),
                Err(PdnError::ResourceLimit { resource: actual, .. }) if actual == resource
            ),
            "resource {resource}"
        );
    }
}

type NormalDocumentCase = (u32, u32, bool, u8, bool, Vec<u8>, Vec<u8>);

fn normal_document_case() -> impl Strategy<Value = NormalDocumentCase> {
    (1_u32..=4, 1_u32..=4).prop_flat_map(|(width, height)| {
        (
            Just(width),
            Just(height),
            any::<bool>(),
            any::<u8>(),
            any::<bool>(),
            prop::collection::vec(0x20_u8..=0x7e, 0..16),
            prop::collection::vec(any::<u8>(), width as usize * height as usize * 4),
        )
    })
}

proptest! {
    #[test]
    fn arbitrary_normal_documents_round_trip_semantically(
        case in normal_document_case(),
    ) {
        let (width, height, visible, opacity, background, name_bytes, channels) = case;
        let name = String::from_utf8(name_bytes).expect("ASCII name");
        let pixels = channels
            .chunks_exact(4)
            .map(|bytes| Bgra::new(bytes[0], bytes[1], bytes[2], bytes[3]))
            .collect::<Vec<_>>();
        let size = Size::new(width, height).expect("size");
        let layer = Layer::new(LayerId::new(1), name, Surface::from_pixels(size, pixels).expect("surface"))
            .with_visibility(visible)
            .with_opacity(opacity)
            .with_background(background);
        let document = Document::from_layers(size, vec![layer]).expect("document");
        let encoded = encode_pdn(&document).expect("encode property document");
        let decoded = decode_pdn(&encoded).expect("decode property document");
        prop_assert_eq!(decoded.document, document);
    }

    #[test]
    fn arbitrary_inputs_never_exceed_configured_pixel_limits(
        bytes in prop::collection::vec(any::<u8>(), 0..256)
    ) {
        if let Ok(decoded) = decode_pdn(&bytes) {
            prop_assert!(decoded.document.size().checked_pixel_count().expect("pixels") <= PdnLimits::default().max_pixels);
        }
    }
}
