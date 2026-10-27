//! Experimental Normal/BGRA PDN writer.
//!
//! Validation is self-round-trip-only at present. Stock Paint.NET 5.1.12
//! acceptance is unverified; semantic tests must not be described as an oracle.

use paint_document::{BlendMode, Document};

use crate::chunks::encode_chunks;
use crate::nrbf::*;
use crate::{PdnError, PdnLimits, PdnUnsupported, PdnWriteOptions};

const DATA_LIBRARY: i32 = 2;
const CORE_LIBRARY: i32 = 3;
const METADATA_TYPE: &str = "System.Collections.Generic.KeyValuePair`2[[System.String, mscorlib],[System.String, mscorlib]][]";

pub(crate) fn encode(
    document: &Document,
    options: &PdnWriteOptions,
    limits: &PdnLimits,
) -> Result<Vec<u8>, PdnError> {
    let size = document.size();
    let pixels = size.checked_pixel_count()?;
    if pixels > limits.max_pixels {
        return Err(resource("pixels", pixels, limits.max_pixels));
    }
    if document.layers().len() > limits.max_layers {
        return Err(resource(
            "layers",
            document.layers().len(),
            limits.max_layers,
        ));
    }
    let mut total_surface_bytes = 0usize;
    for layer in document.layers() {
        if layer.blend_mode() != BlendMode::Normal {
            return Err(PdnError::Unsupported(PdnUnsupported::BlendOperation(
                format!("{:?}", layer.blend_mode()),
            )));
        }
        total_surface_bytes = total_surface_bytes
            .checked_add(layer.surface().size().checked_buffer_len()?)
            .ok_or_else(|| malformed("total surface size overflow"))?;
        if total_surface_bytes > limits.max_total_surface_bytes {
            return Err(resource(
                "total surface bytes",
                total_surface_bytes,
                limits.max_total_surface_bytes,
            ));
        }
    }
    let width = i32::try_from(size.width()).map_err(|_| unsupported_dimension())?;
    let height = i32::try_from(size.height()).map_err(|_| unsupported_dimension())?;
    let layer_count = i32::try_from(document.layers().len())
        .map_err(|_| malformed("layer count exceeds the PDN field width"))?;

    let mut graph = GraphBuilder::new();
    let document_id = graph.id()?;
    let layer_list_id = graph.id()?;
    let version_id = graph.id()?;
    let metadata_id = graph.id()?;
    let items_id = graph.id()?;
    graph.array(
        metadata_id,
        ArrayEncoding::Binary(BinaryArrayType::Single),
        BinaryType::SystemClass(METADATA_TYPE.into()),
        vec![],
    );
    graph.class(
        version_id,
        "System.Version",
        None,
        vec![
            member(
                "_Major",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(0),
            ),
            member(
                "_Minor",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(1),
            ),
            member(
                "_Build",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(0),
            ),
            member(
                "_Revision",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(0),
            ),
        ],
    )?;

    let mut layer_ids = Vec::new();
    layer_ids
        .try_reserve_exact(document.layers().len())
        .map_err(|_| PdnError::Allocation {
            context: "writer layer table",
            requested: document
                .layers()
                .len()
                .saturating_mul(std::mem::size_of::<ObjectId>()),
        })?;
    let mut schemas = None;
    for layer in document.layers() {
        let bitmap_id = graph.id()?;
        let bitmap_properties_id = graph.id()?;
        let surface_id = graph.id()?;
        let properties_id = graph.id()?;
        let blend_operation_id = graph.id()?;
        let memory_id = graph.id()?;
        let name_id = graph.id()?;
        let blend_mode_id = graph.id()?;
        graph.string(name_id, layer.name());
        let bitmap_values = vec![
            reference(bitmap_properties_id),
            reference(surface_id),
            boolean(false),
            int32(width),
            int32(height),
            reference(properties_id),
        ];
        let bitmap_properties_values = vec![reference(blend_operation_id)];
        let surface_values = vec![
            int32(width),
            int32(height),
            int32(width.checked_mul(4).ok_or_else(unsupported_dimension)?),
            reference(memory_id),
        ];
        let storage_length = i64::from(width)
            .checked_mul(4)
            .and_then(|stride| stride.checked_mul(i64::from(height)))
            .ok_or_else(unsupported_dimension)?;
        let memory_values = vec![
            primitive(PrimitiveValue::Int64(storage_length)),
            boolean(false),
            boolean(true),
        ];
        let properties_values = vec![
            reference(name_id),
            reference(metadata_id),
            boolean(layer.is_visible()),
            boolean(layer.is_background()),
            primitive(PrimitiveValue::Byte(layer.opacity())),
            reference(blend_mode_id),
        ];
        let blend_mode_values = vec![int32(0)];
        if let Some(schema_ids) = schemas {
            let (
                bitmap,
                bitmap_properties,
                surface,
                properties,
                blend_operation,
                memory,
                blend_mode,
            ): (
                ObjectId,
                ObjectId,
                ObjectId,
                ObjectId,
                ObjectId,
                ObjectId,
                ObjectId,
            ) = schema_ids;
            graph.instance(bitmap_id, bitmap, bitmap_values)?;
            graph.instance(
                bitmap_properties_id,
                bitmap_properties,
                bitmap_properties_values,
            )?;
            graph.instance(surface_id, surface, surface_values)?;
            graph.instance(properties_id, properties, properties_values)?;
            graph.instance(blend_operation_id, blend_operation, vec![])?;
            graph.instance(memory_id, memory, memory_values)?;
            graph.instance(blend_mode_id, blend_mode, blend_mode_values)?;
        } else {
            graph.class(
                bitmap_id,
                "PaintDotNet.BitmapLayer",
                Some(DATA_LIBRARY),
                vec![
                    member(
                        "properties",
                        data_class("PaintDotNet.BitmapLayer+BitmapLayerProperties"),
                        reference(bitmap_properties_id),
                    ),
                    member(
                        "surface",
                        core_class("PaintDotNet.Surface"),
                        reference(surface_id),
                    ),
                    member(
                        "Layer+isDisposed",
                        BinaryType::Primitive(PrimitiveType::Boolean),
                        boolean(false),
                    ),
                    member(
                        "Layer+width",
                        BinaryType::Primitive(PrimitiveType::Int32),
                        int32(width),
                    ),
                    member(
                        "Layer+height",
                        BinaryType::Primitive(PrimitiveType::Int32),
                        int32(height),
                    ),
                    member(
                        "Layer+properties",
                        data_class("PaintDotNet.Layer+LayerProperties"),
                        reference(properties_id),
                    ),
                ],
            )?;
            graph.class(
                bitmap_properties_id,
                "PaintDotNet.BitmapLayer+BitmapLayerProperties",
                Some(DATA_LIBRARY),
                vec![member(
                    "blendOp",
                    data_class("PaintDotNet.UserBlendOps+NormalBlendOp"),
                    reference(blend_operation_id),
                )],
            )?;
            graph.class(
                surface_id,
                "PaintDotNet.Surface",
                Some(CORE_LIBRARY),
                vec![
                    member(
                        "width",
                        BinaryType::Primitive(PrimitiveType::Int32),
                        int32(width),
                    ),
                    member(
                        "height",
                        BinaryType::Primitive(PrimitiveType::Int32),
                        int32(height),
                    ),
                    member(
                        "stride",
                        BinaryType::Primitive(PrimitiveType::Int32),
                        int32(width * 4),
                    ),
                    member(
                        "scan0",
                        core_class("PaintDotNet.MemoryBlock"),
                        reference(memory_id),
                    ),
                ],
            )?;
            graph.class(
                properties_id,
                "PaintDotNet.Layer+LayerProperties",
                Some(DATA_LIBRARY),
                vec![
                    member("name", BinaryType::String, reference(name_id)),
                    member(
                        "userMetadataItems",
                        BinaryType::SystemClass(METADATA_TYPE.into()),
                        reference(metadata_id),
                    ),
                    member(
                        "visible",
                        BinaryType::Primitive(PrimitiveType::Boolean),
                        boolean(layer.is_visible()),
                    ),
                    member(
                        "isBackground",
                        BinaryType::Primitive(PrimitiveType::Boolean),
                        boolean(layer.is_background()),
                    ),
                    member(
                        "opacity",
                        BinaryType::Primitive(PrimitiveType::Byte),
                        primitive(PrimitiveValue::Byte(layer.opacity())),
                    ),
                    member(
                        "blendMode",
                        data_class("PaintDotNet.LayerBlendMode"),
                        reference(blend_mode_id),
                    ),
                ],
            )?;
            graph.class(
                blend_operation_id,
                "PaintDotNet.UserBlendOps+NormalBlendOp",
                Some(DATA_LIBRARY),
                vec![],
            )?;
            graph.class(
                memory_id,
                "PaintDotNet.MemoryBlock",
                Some(CORE_LIBRARY),
                vec![
                    member(
                        "length64",
                        BinaryType::Primitive(PrimitiveType::Int64),
                        primitive(PrimitiveValue::Int64(storage_length)),
                    ),
                    member(
                        "hasParent",
                        BinaryType::Primitive(PrimitiveType::Boolean),
                        boolean(false),
                    ),
                    member(
                        "deferred",
                        BinaryType::Primitive(PrimitiveType::Boolean),
                        boolean(true),
                    ),
                ],
            )?;
            graph.class(
                blend_mode_id,
                "PaintDotNet.LayerBlendMode",
                Some(DATA_LIBRARY),
                vec![member(
                    "value__",
                    BinaryType::Primitive(PrimitiveType::Int32),
                    int32(0),
                )],
            )?;
            schemas = Some((
                bitmap_id,
                bitmap_properties_id,
                surface_id,
                properties_id,
                blend_operation_id,
                memory_id,
                blend_mode_id,
            ));
        }
        layer_ids.push(bitmap_id);
    }
    graph.array(
        items_id,
        ArrayEncoding::SingleObject,
        BinaryType::Object,
        layer_ids.iter().copied().map(reference).collect(),
    );
    graph.class(
        layer_list_id,
        "PaintDotNet.LayerList",
        Some(DATA_LIBRARY),
        vec![
            member(
                "parent",
                data_class("PaintDotNet.Document"),
                reference(document_id),
            ),
            member(
                "ArrayList+_items",
                BinaryType::ObjectArray,
                reference(items_id),
            ),
            member(
                "ArrayList+_size",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(layer_count),
            ),
            member(
                "ArrayList+_version",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(layer_count),
            ),
        ],
    )?;
    graph.class(
        document_id,
        "PaintDotNet.Document",
        Some(DATA_LIBRARY),
        vec![
            member(
                "isDisposed",
                BinaryType::Primitive(PrimitiveType::Boolean),
                boolean(false),
            ),
            member(
                "layers",
                data_class("PaintDotNet.LayerList"),
                reference(layer_list_id),
            ),
            member(
                "width",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(width),
            ),
            member(
                "height",
                BinaryType::Primitive(PrimitiveType::Int32),
                int32(height),
            ),
            member(
                "savedWith",
                BinaryType::SystemClass("System.Version".into()),
                reference(version_id),
            ),
            member(
                "userMetadataItems",
                BinaryType::SystemClass(METADATA_TYPE.into()),
                reference(metadata_id),
            ),
        ],
    )?;

    let xml = format!(
        "<pdnImage width=\"{}\" height=\"{}\" layers=\"{}\" savedWithVersion=\"0.1.0.0\"><custom /></pdnImage>",
        size.width(),
        size.height(),
        document.layers().len()
    );
    if xml.len() > limits.max_header_bytes || xml.len() > 0x00ff_ffff {
        return Err(resource(
            "header bytes",
            xml.len(),
            limits.max_header_bytes.min(0x00ff_ffff),
        ));
    }
    let nrbf = write_nrbf(&graph.stream, &limits.nrbf)?;
    let mut output = Vec::new();
    append(&mut output, b"PDN3", limits)?;
    let xml_length = (xml.len() as u32).to_le_bytes();
    append(&mut output, &xml_length[..3], limits)?;
    append(&mut output, xml.as_bytes(), limits)?;
    append(&mut output, &[0, 1], limits)?;
    append(&mut output, &nrbf, limits)?;
    for layer in document.layers() {
        let pixels = layer.surface().to_bgra_bytes()?;
        let chunks = encode_chunks(&pixels, options, limits)?;
        append(&mut output, &chunks, limits)?;
    }
    Ok(output)
}

fn member(name: &str, binary_type: BinaryType, value: NrbfValue) -> (&str, BinaryType, NrbfValue) {
    (name, binary_type, value)
}

fn data_class(name: &str) -> BinaryType {
    BinaryType::Class {
        name: name.into(),
        library_id: DATA_LIBRARY,
    }
}

fn core_class(name: &str) -> BinaryType {
    BinaryType::Class {
        name: name.into(),
        library_id: CORE_LIBRARY,
    }
}

fn reference(id: ObjectId) -> NrbfValue {
    NrbfValue::Reference(id)
}

fn int32(value: i32) -> NrbfValue {
    primitive(PrimitiveValue::Int32(value))
}

fn boolean(value: bool) -> NrbfValue {
    primitive(PrimitiveValue::Boolean(value))
}

fn primitive(value: PrimitiveValue) -> NrbfValue {
    NrbfValue::Primitive(value)
}

struct GraphBuilder {
    stream: NrbfStream,
    next_id: ObjectId,
}

impl GraphBuilder {
    fn new() -> Self {
        let mut stream = NrbfStream::new(1, -1);
        stream
            .libraries
            .insert(DATA_LIBRARY, "PaintDotNet.Data".into());
        stream
            .libraries
            .insert(CORE_LIBRARY, "PaintDotNet.Core".into());
        Self { stream, next_id: 1 }
    }

    fn id(&mut self) -> Result<ObjectId, PdnError> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| malformed("NRBF object id overflow"))?;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    fn class(
        &mut self,
        id: ObjectId,
        name: &str,
        library_id: Option<i32>,
        members: Vec<(&str, BinaryType, NrbfValue)>,
    ) -> Result<(), PdnError> {
        let (names, types, values) = split_members(members);
        self.stream.schemas.insert(
            id,
            ClassSchema {
                name: name.into(),
                member_names: names,
                member_types: Some(types),
                library_id,
            },
        );
        self.instance(id, id, values)
    }

    fn instance(
        &mut self,
        id: ObjectId,
        schema_id: ObjectId,
        members: Vec<NrbfValue>,
    ) -> Result<(), PdnError> {
        if self.stream.objects.contains_key(&id) {
            return Err(malformed("duplicate writer object id"));
        }
        self.stream.objects.insert(
            id,
            NrbfObject::Class(NrbfClass {
                object_id: id,
                schema_id,
                members,
            }),
        );
        Ok(())
    }

    fn string(&mut self, id: ObjectId, value: &str) {
        self.stream
            .objects
            .insert(id, NrbfObject::String(value.into()));
    }

    fn array(
        &mut self,
        id: ObjectId,
        encoding: ArrayEncoding,
        element_type: BinaryType,
        items: Vec<NrbfValue>,
    ) {
        let length = items.len();
        self.stream.objects.insert(
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
}

fn split_members(
    members: Vec<(&str, BinaryType, NrbfValue)>,
) -> (Vec<String>, Vec<BinaryType>, Vec<NrbfValue>) {
    let mut names = Vec::with_capacity(members.len());
    let mut types = Vec::with_capacity(members.len());
    let mut values = Vec::with_capacity(members.len());
    for (name, binary_type, value) in members {
        names.push(name.into());
        types.push(binary_type);
        values.push(value);
    }
    (names, types, values)
}

fn append(output: &mut Vec<u8>, bytes: &[u8], limits: &PdnLimits) -> Result<(), PdnError> {
    let length = output
        .len()
        .checked_add(bytes.len())
        .ok_or_else(|| malformed("PDN output length overflow"))?;
    if length > limits.max_output_bytes {
        return Err(resource("output bytes", length, limits.max_output_bytes));
    }
    output
        .try_reserve(bytes.len())
        .map_err(|_| PdnError::Allocation {
            context: "PDN output",
            requested: length,
        })?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn unsupported_dimension() -> PdnError {
    PdnError::Unsupported(PdnUnsupported::SurfaceLayout(
        "dimensions exceed PDN 32-bit fields",
    ))
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
