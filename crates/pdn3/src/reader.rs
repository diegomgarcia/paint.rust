use std::collections::{BTreeMap, BTreeSet};

use paint_core::{Bgra, Size, Surface};
use paint_document::{BlendMode, Document, Layer, LayerId};

use crate::chunks::{ChunkReader, decode_chunks};
use crate::nrbf::{
    NrbfArray, NrbfClass, NrbfObject, NrbfStream, NrbfValue, ObjectId, PrimitiveValue,
    parse_nrbf_prefix,
};
use crate::xml::{RootXml, parse_header};
use crate::{DecodedPdn, PdnError, PdnLimits, PdnReport, PdnUnmappedMember, PdnUnsupported};

const DOCUMENT: &str = "PaintDotNet.Document";
const LAYER_LIST: &str = "PaintDotNet.LayerList";
const BITMAP_LAYER: &str = "PaintDotNet.BitmapLayer";
const BITMAP_PROPERTIES: &str = "PaintDotNet.BitmapLayer+BitmapLayerProperties";
const SURFACE: &str = "PaintDotNet.Surface";
const MEMORY_BLOCK: &str = "PaintDotNet.MemoryBlock";
const LAYER_PROPERTIES: &str = "PaintDotNet.Layer+LayerProperties";
const BLEND_MODE: &str = "PaintDotNet.LayerBlendMode";
const NORMAL_BLEND: &str = "PaintDotNet.UserBlendOps+NormalBlendOp";

pub(crate) fn decode(bytes: &[u8], limits: &PdnLimits) -> Result<DecodedPdn, PdnError> {
    if bytes.len() > limits.max_input_bytes {
        return Err(resource("input bytes", bytes.len(), limits.max_input_bytes));
    }
    if bytes.len() < 4 {
        return Err(PdnError::Truncated {
            offset: bytes.len(),
            context: "PDN3 magic",
        });
    }
    if &bytes[..4] != b"PDN3" {
        return Err(PdnError::InvalidMagic);
    }
    let length_bytes = bytes.get(4..7).ok_or(PdnError::Truncated {
        offset: bytes.len(),
        context: "24-bit XML length",
    })?;
    let xml_length = length_bytes[0] as usize
        | (length_bytes[1] as usize) << 8
        | (length_bytes[2] as usize) << 16;
    if xml_length > limits.max_header_bytes {
        return Err(resource(
            "header bytes",
            xml_length,
            limits.max_header_bytes,
        ));
    }
    let xml_end = 7usize
        .checked_add(xml_length)
        .ok_or_else(|| malformed("XML end offset overflow"))?;
    let xml_bytes = bytes.get(7..xml_end).ok_or(PdnError::Truncated {
        offset: bytes.len(),
        context: "XML header",
    })?;
    let header_xml = std::str::from_utf8(xml_bytes)
        .map_err(|_| PdnError::XmlUtf8)?
        .to_owned();
    let xml = parse_header(&header_xml, limits)?;
    if bytes.get(xml_end..xml_end + 2) != Some(&[0, 1]) {
        return Err(malformed("missing PDN 00 01 data indicator"));
    }
    let nrbf_start = xml_end + 2;
    let parsed = parse_nrbf_prefix(&bytes[nrbf_start..], &limits.nrbf)?;
    let chunk_start = nrbf_start
        .checked_add(parsed.consumed)
        .ok_or_else(|| malformed("pixel chunk offset overflow"))?;
    if chunk_start > bytes.len() {
        return Err(malformed("NRBF consumed beyond the PDN input"));
    }
    let stream = parsed.stream;
    let mut report = PdnReport::default();
    let (document, producer_version, consumed_chunks) =
        decode_document(&stream, &xml, &bytes[chunk_start..], limits, &mut report)?;
    debug_assert_eq!(document.layers().len(), xml.layers);
    let trailing_bytes = bytes[chunk_start + consumed_chunks..].to_vec();
    report.trailing_bytes = trailing_bytes.len();
    report.unmapped_objects = unreachable_objects(&stream);
    Ok(DecodedPdn {
        document,
        header_xml,
        producer_version,
        nrbf: stream,
        report,
        trailing_bytes,
    })
}

fn decode_document(
    stream: &NrbfStream,
    xml: &RootXml,
    chunk_bytes: &[u8],
    limits: &PdnLimits,
    report: &mut PdnReport,
) -> Result<(Document, String, usize), PdnError> {
    let (_, root) = expect_class(stream, &NrbfValue::Reference(stream.root_id), DOCUMENT)?;
    record_unmapped(
        stream,
        root,
        &[
            "isDisposed",
            "layers",
            "width",
            "height",
            "savedWith",
            "userMetadataItems",
        ],
        report,
    );
    if boolean(member(stream, root, "isDisposed")?)? {
        return Err(malformed("disposed PDN document"));
    }
    let width = positive_u32(member(stream, root, "width")?, "document width")?;
    let height = positive_u32(member(stream, root, "height")?, "document height")?;
    if width != xml.width || height != xml.height {
        return Err(malformed("XML and NRBF document dimensions differ"));
    }
    let size = Size::new(width, height)?;
    let pixels = size.checked_pixel_count()?;
    if pixels > limits.max_pixels {
        return Err(resource("pixels", pixels, limits.max_pixels));
    }
    report.document_metadata_entries =
        metadata_count(stream, member(stream, root, "userMetadataItems")?)?;
    let producer_version = decode_version(stream, member(stream, root, "savedWith")?)?;
    if producer_version != xml.saved_with {
        return Err(malformed("XML and NRBF producer versions differ"));
    }
    let (_, layers) = expect_class(stream, member(stream, root, "layers")?, LAYER_LIST)?;
    record_unmapped(
        stream,
        layers,
        &[
            "parent",
            "ArrayList+_items",
            "ArrayList+_size",
            "ArrayList+_version",
        ],
        report,
    );
    let (_, items) = expect_array(stream, member(stream, layers, "ArrayList+_items")?)?;
    let layer_count = nonnegative_usize(member(stream, layers, "ArrayList+_size")?, "layer count")?;
    if layer_count != xml.layers || layer_count > items.items.len() {
        return Err(malformed("XML, NRBF, and layer array counts differ"));
    }
    if layer_count > limits.max_layers {
        return Err(resource("layers", layer_count, limits.max_layers));
    }
    if layer_count == 0 {
        return Err(malformed("PDN document has no layers"));
    }
    let mut chunk_reader = ChunkReader::new(chunk_bytes);
    let mut surface_layers = BTreeMap::<ObjectId, usize>::new();
    let mut canonical_layers = Vec::<Layer>::new();
    canonical_layers
        .try_reserve_exact(layer_count)
        .map_err(|_| PdnError::Allocation {
            context: "layer table",
            requested: layer_count.saturating_mul(std::mem::size_of::<Layer>()),
        })?;
    let canonical_bytes = size.checked_buffer_len()?;
    let mut total_surface_bytes = 0usize;
    for index in 0..layer_count {
        let value = items
            .items
            .get(index)
            .ok_or_else(|| malformed("missing layer item"))?;
        let (_, layer) = expect_class(stream, value, BITMAP_LAYER)
            .map_err(|_| PdnError::Unsupported(PdnUnsupported::NonBitmapLayer))?;
        record_unmapped(
            stream,
            layer,
            &[
                "properties",
                "surface",
                "Layer+isDisposed",
                "Layer+width",
                "Layer+height",
                "Layer+properties",
            ],
            report,
        );
        if boolean(member(stream, layer, "Layer+isDisposed")?)? {
            return Err(malformed("disposed bitmap layer"));
        }
        if positive_u32(member(stream, layer, "Layer+width")?, "layer width")? != width
            || positive_u32(member(stream, layer, "Layer+height")?, "layer height")? != height
        {
            return Err(malformed("layer and document dimensions differ"));
        }
        let (_, properties) = expect_class(
            stream,
            member(stream, layer, "Layer+properties")?,
            LAYER_PROPERTIES,
        )?;
        record_unmapped(
            stream,
            properties,
            &[
                "name",
                "userMetadataItems",
                "visible",
                "isBackground",
                "opacity",
                "blendMode",
            ],
            report,
        );
        let name = string(stream, member(stream, properties, "name")?)?.to_owned();
        let visible = boolean(member(stream, properties, "visible")?)?;
        let background = boolean(member(stream, properties, "isBackground")?)?;
        let opacity = byte(member(stream, properties, "opacity")?)?;
        report.layer_metadata_entries.push(metadata_count(
            stream,
            member(stream, properties, "userMetadataItems")?,
        )?);
        let blend = integer(
            stream,
            member(stream, properties, "blendMode")?,
            BLEND_MODE,
            "value__",
        )?;
        if blend != 0 {
            return Err(PdnError::Unsupported(PdnUnsupported::BlendMode(blend)));
        }
        let (_, bitmap_properties) = expect_class(
            stream,
            member(stream, layer, "properties")?,
            BITMAP_PROPERTIES,
        )?;
        record_unmapped(stream, bitmap_properties, &["blendOp"], report);
        let (_, blend_operation) = class(stream, member(stream, bitmap_properties, "blendOp")?)?;
        let blend_name = schema(stream, blend_operation)?.name.clone();
        if blend_name != NORMAL_BLEND {
            return Err(PdnError::Unsupported(PdnUnsupported::BlendOperation(
                blend_name,
            )));
        }
        record_unmapped(stream, blend_operation, &[], report);

        let (_, surface_class) = expect_class(stream, member(stream, layer, "surface")?, SURFACE)?;
        record_unmapped(
            stream,
            surface_class,
            &["width", "height", "stride", "scan0"],
            report,
        );
        if positive_u32(member(stream, surface_class, "width")?, "surface width")? != width
            || positive_u32(member(stream, surface_class, "height")?, "surface height")? != height
        {
            return Err(malformed("surface and document dimensions differ"));
        }
        let stride = nonnegative_usize(member(stream, surface_class, "stride")?, "surface stride")?;
        let packed_stride = usize::try_from(width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or_else(|| malformed("packed surface stride overflow"))?;
        if stride < packed_stride {
            return Err(PdnError::Unsupported(PdnUnsupported::SurfaceLayout(
                "stride is smaller than packed BGRA rows",
            )));
        }
        let storage_length = stride
            .checked_mul(height as usize)
            .ok_or_else(|| malformed("surface storage length overflow"))?;
        if storage_length > limits.max_surface_bytes {
            return Err(resource(
                "surface bytes",
                storage_length,
                limits.max_surface_bytes,
            ));
        }
        let (memory_id, memory) = expect_class(
            stream,
            member(stream, surface_class, "scan0")?,
            MEMORY_BLOCK,
        )?;
        record_unmapped(
            stream,
            memory,
            &["length64", "hasParent", "deferred"],
            report,
        );
        let declared_length =
            nonnegative_usize(member(stream, memory, "length64")?, "memory length")?;
        if declared_length != storage_length {
            return Err(PdnError::Unsupported(PdnUnsupported::SurfaceLayout(
                "memory length differs from stride times height",
            )));
        }
        let has_parent = boolean(member(stream, memory, "hasParent")?)?;
        let _deferred = boolean(member(stream, memory, "deferred")?)?;
        let shared_from = surface_layers.get(&memory_id).copied();
        if shared_from.is_none() && has_parent {
            return Err(PdnError::Unsupported(
                PdnUnsupported::SharedSurfaceWithoutDecodedParent,
            ));
        }
        total_surface_bytes = total_surface_bytes
            .checked_add(canonical_bytes)
            .ok_or_else(|| malformed("total surface byte count overflow"))?;
        if total_surface_bytes > limits.max_total_surface_bytes {
            return Err(resource(
                "total surface bytes",
                total_surface_bytes,
                limits.max_total_surface_bytes,
            ));
        }
        let canonical_index = canonical_layers.len();
        let surface = if let Some(canonical_index) = shared_from {
            report.shared_layers.push(index);
            canonical_layers[canonical_index].surface().clone()
        } else {
            let raw = decode_chunks(&mut chunk_reader, storage_length, limits)?;
            let surface = surface_from_bgr_rows(size, raw, stride)?;
            if stride != packed_stride {
                report.padded_layers.push(index);
            }
            surface
        };
        let id =
            LayerId::new(u64::try_from(index + 1).map_err(|_| malformed("layer id overflow"))?);
        canonical_layers.push(
            Layer::new(id, name, surface)
                .with_visibility(visible)
                .with_opacity(opacity)
                .with_background(background),
        );
        if shared_from.is_none() {
            surface_layers.insert(memory_id, canonical_index);
        }
    }
    let document = Document::from_layers(size, canonical_layers)?;
    debug_assert!(
        document
            .layers()
            .iter()
            .all(|layer| layer.blend_mode() == BlendMode::Normal)
    );
    Ok((document, producer_version, chunk_reader.position()))
}

fn decode_version(stream: &NrbfStream, value: &NrbfValue) -> Result<String, PdnError> {
    let (_, version) = expect_class(stream, value, "System.Version")?;
    let major = integer_value(member(stream, version, "_Major")?)?;
    let minor = integer_value(member(stream, version, "_Minor")?)?;
    let build = integer_value(member(stream, version, "_Build")?)?;
    let revision = integer_value(member(stream, version, "_Revision")?)?;
    if [major, minor, build, revision].iter().any(|part| *part < 0) {
        return Err(malformed("negative producer version component"));
    }
    Ok(format!("{major}.{minor}.{build}.{revision}"))
}

fn metadata_count(stream: &NrbfStream, value: &NrbfValue) -> Result<usize, PdnError> {
    match value {
        NrbfValue::Null => Ok(0),
        NrbfValue::Reference(_) => Ok(expect_array(stream, value)?.1.items.len()),
        NrbfValue::Primitive(_) => Err(malformed("metadata field is not an array")),
    }
}

fn surface_from_bgr_rows(size: Size, raw: Vec<u8>, stride: usize) -> Result<Surface, PdnError> {
    let width = size.width() as usize;
    let height = size.height() as usize;
    let count = size.checked_pixel_count()?;
    let requested = size.checked_buffer_len()?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| PdnError::Allocation {
            context: "BGRA layer pixels",
            requested,
        })?;
    for row in 0..height {
        let start = row
            .checked_mul(stride)
            .ok_or_else(|| malformed("surface row overflow"))?;
        let row_bytes = raw
            .get(start..start + width * 4)
            .ok_or_else(|| malformed("surface row exceeds decoded storage"))?;
        for pixel in row_bytes.chunks_exact(4) {
            pixels.push(Bgra::new(pixel[0], pixel[1], pixel[2], pixel[3]));
        }
    }
    Surface::from_pixels(size, pixels).map_err(Into::into)
}

fn record_unmapped(stream: &NrbfStream, class: &NrbfClass, known: &[&str], report: &mut PdnReport) {
    if let Ok(schema) = schema(stream, class) {
        for name in &schema.member_names {
            if !known.contains(&name.as_str()) {
                report.unmapped_members.push(PdnUnmappedMember {
                    object_id: class.object_id,
                    class_name: schema.name.clone(),
                    member_name: name.clone(),
                });
            }
        }
    }
}

fn unreachable_objects(stream: &NrbfStream) -> Vec<ObjectId> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![stream.root_id];
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(object) = stream.objects.get(&id) {
            match object {
                NrbfObject::Class(class) => {
                    pending.extend(class.members.iter().filter_map(reference_id))
                }
                NrbfObject::Array(array) => {
                    pending.extend(array.items.iter().filter_map(reference_id))
                }
                NrbfObject::String(_) => {}
            }
        }
    }
    stream
        .objects
        .keys()
        .filter(|id| !seen.contains(id))
        .copied()
        .collect()
}

fn reference_id(value: &NrbfValue) -> Option<ObjectId> {
    match value {
        NrbfValue::Reference(id) => Some(*id),
        _ => None,
    }
}

fn class<'a>(
    stream: &'a NrbfStream,
    value: &NrbfValue,
) -> Result<(ObjectId, &'a NrbfClass), PdnError> {
    let id = match value {
        NrbfValue::Reference(id) => *id,
        _ => return Err(malformed("expected an NRBF class reference")),
    };
    match stream.objects.get(&id) {
        Some(NrbfObject::Class(class)) => Ok((id, class)),
        _ => Err(malformed("expected an NRBF class object")),
    }
}

fn expect_class<'a>(
    stream: &'a NrbfStream,
    value: &NrbfValue,
    expected: &str,
) -> Result<(ObjectId, &'a NrbfClass), PdnError> {
    let (id, class) = class(stream, value)?;
    let actual = schema(stream, class)?.name.clone();
    if actual != expected {
        return Err(PdnError::Unsupported(PdnUnsupported::Class(actual)));
    }
    Ok((id, class))
}

fn schema<'a>(
    stream: &'a NrbfStream,
    class: &NrbfClass,
) -> Result<&'a crate::nrbf::ClassSchema, PdnError> {
    stream
        .schemas
        .get(&class.schema_id)
        .ok_or_else(|| malformed("class schema is missing"))
}

fn member<'a>(
    stream: &NrbfStream,
    class: &'a NrbfClass,
    name: &str,
) -> Result<&'a NrbfValue, PdnError> {
    let schema = schema(stream, class)?;
    if schema.member_names.len() != class.members.len() {
        return Err(malformed("class schema and member counts differ"));
    }
    let index = schema
        .member_names
        .iter()
        .position(|candidate| candidate == name)
        .ok_or_else(|| malformed("required PDN class member is missing"))?;
    Ok(&class.members[index])
}

fn integer(
    stream: &NrbfStream,
    value: &NrbfValue,
    class_name: &str,
    member_name: &str,
) -> Result<i32, PdnError> {
    let (_, class) = expect_class(stream, value, class_name)?;
    integer_value(member(stream, class, member_name)?)
}

fn integer_value(value: &NrbfValue) -> Result<i32, PdnError> {
    match value {
        NrbfValue::Primitive(PrimitiveValue::Int32(value)) => Ok(*value),
        _ => Err(malformed("expected a 32-bit integer")),
    }
}

fn positive_u32(value: &NrbfValue, context: &'static str) -> Result<u32, PdnError> {
    let value = integer_value(value)?;
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| malformed(context))
}

fn nonnegative_usize(value: &NrbfValue, context: &'static str) -> Result<usize, PdnError> {
    match value {
        NrbfValue::Primitive(PrimitiveValue::Int32(value)) => usize::try_from(*value),
        NrbfValue::Primitive(PrimitiveValue::Int64(value)) => usize::try_from(*value),
        _ => return Err(malformed(context)),
    }
    .map_err(|_| malformed(context))
}

fn boolean(value: &NrbfValue) -> Result<bool, PdnError> {
    match value {
        NrbfValue::Primitive(PrimitiveValue::Boolean(value)) => Ok(*value),
        _ => Err(malformed("expected a Boolean")),
    }
}

fn byte(value: &NrbfValue) -> Result<u8, PdnError> {
    match value {
        NrbfValue::Primitive(PrimitiveValue::Byte(value)) => Ok(*value),
        _ => Err(malformed("expected a byte")),
    }
}

fn string<'a>(stream: &'a NrbfStream, value: &NrbfValue) -> Result<&'a str, PdnError> {
    match value {
        NrbfValue::Reference(id) => match stream.objects.get(id) {
            Some(NrbfObject::String(value)) => Ok(value),
            _ => Err(malformed("expected an NRBF string")),
        },
        _ => Err(malformed("expected an NRBF string reference")),
    }
}

fn expect_array<'a>(
    stream: &'a NrbfStream,
    value: &NrbfValue,
) -> Result<(ObjectId, &'a NrbfArray), PdnError> {
    let id = match value {
        NrbfValue::Reference(id) => *id,
        _ => return Err(malformed("expected an NRBF array reference")),
    };
    match stream.objects.get(&id) {
        Some(NrbfObject::Array(array)) => Ok((id, array)),
        _ => Err(malformed("expected an NRBF array")),
    }
}

fn malformed(reason: &'static str) -> PdnError {
    PdnError::Malformed { offset: 0, reason }
}

fn resource(resource_name: &'static str, requested: usize, maximum: usize) -> PdnError {
    PdnError::ResourceLimit {
        resource: resource_name,
        requested: requested as u64,
        maximum,
    }
}
