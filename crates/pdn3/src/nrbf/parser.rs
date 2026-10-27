use super::*;

pub(super) fn parse(input: &[u8], limits: &NrbfLimits) -> Result<ParsedNrbf, NrbfError> {
    Parser::new(input, limits).parse_stream()
}

#[derive(Clone, Copy, Debug)]
enum BinaryKind {
    Primitive,
    String,
    Object,
    SystemClass,
    Class,
    ObjectArray,
    StringArray,
    PrimitiveArray,
}

enum RecordOutcome {
    Value(NrbfValue),
    Library,
    Nulls(usize),
    MessageEnd,
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
    readable: usize,
    limits: &'a NrbfLimits,
    records: usize,
    ids: BTreeSet<ObjectId>,
    stream: NrbfStream,
}

impl<'a> Parser<'a> {
    fn new(input: &'a [u8], limits: &'a NrbfLimits) -> Self {
        Self {
            input,
            position: 0,
            readable: input.len().min(limits.max_stream_bytes),
            limits,
            records: 0,
            ids: BTreeSet::new(),
            stream: NrbfStream::new(0, 0),
        }
    }

    fn parse_stream(mut self) -> Result<ParsedNrbf, NrbfError> {
        let header_offset = self.position;
        self.bump_records()?;
        if self.read_u8()? != 0 {
            return Err(self.malformed("missing serialization header"));
        }
        self.stream.root_id = self.read_i32()?;
        self.stream.header_id = self.read_i32()?;
        let major = self.read_i32()?;
        let minor = self.read_i32()?;
        if major != 1 || minor != 0 {
            return Err(NrbfError::Unsupported {
                offset: header_offset,
                feature: "NRBF serialization version other than 1.0",
            });
        }
        if self.stream.root_id <= 0 {
            return Err(NrbfError::Unsupported {
                offset: header_offset,
                feature: "method-call streams or non-object roots",
            });
        }

        loop {
            match self.read_record(0)? {
                RecordOutcome::MessageEnd => break,
                RecordOutcome::Library | RecordOutcome::Value(_) => {}
                RecordOutcome::Nulls(_) => {
                    return Err(self.malformed("top-level null run has no member context"));
                }
            }
        }
        self.validate_references()?;
        Ok(ParsedNrbf {
            stream: self.stream,
            consumed: self.position,
        })
    }

    fn read_record(&mut self, depth: usize) -> Result<RecordOutcome, NrbfError> {
        if depth > self.limits.max_depth {
            return Err(self.limit("object depth", depth, self.limits.max_depth));
        }
        let record_offset = self.position;
        self.bump_records()?;
        let record_type = self.read_u8()?;
        match record_type {
            0 => Err(self.malformed("duplicate serialization header")),
            1 => self.read_class_with_id(depth),
            2..=5 => self.read_class_definition(record_type, depth),
            6 => {
                let id = self.read_i32()?;
                self.register_id(id)?;
                let value = self.read_string()?;
                self.insert_object(id, NrbfObject::String(value))?;
                Ok(RecordOutcome::Value(NrbfValue::Reference(id)))
            }
            7 => self.read_binary_array(depth),
            8 => {
                let primitive_type = self.read_primitive_type()?;
                if primitive_type == PrimitiveType::String {
                    return Err(self.malformed("typed primitive record cannot contain a string"));
                }
                let value = self.read_primitive(primitive_type)?;
                Ok(RecordOutcome::Value(NrbfValue::Primitive(value)))
            }
            9 => {
                let id = self.read_i32()?;
                if id <= 0 {
                    return Err(self.malformed("object reference id must be positive"));
                }
                Ok(RecordOutcome::Value(NrbfValue::Reference(id)))
            }
            10 => Ok(RecordOutcome::Value(NrbfValue::Null)),
            11 => Ok(RecordOutcome::MessageEnd),
            12 => {
                let id = self.read_i32()?;
                let name = self.read_string()?;
                if self.stream.libraries.insert(id, name).is_some() {
                    return Err(self.malformed("duplicate binary library id"));
                }
                Ok(RecordOutcome::Library)
            }
            13 => {
                let count = usize::from(self.read_u8()?);
                Ok(RecordOutcome::Nulls(count))
            }
            14 => {
                let count = self.read_nonnegative_count("null run length")?;
                Ok(RecordOutcome::Nulls(count))
            }
            15 => self.read_single_array(depth, ArrayEncoding::SinglePrimitive),
            16 => self.read_single_array(depth, ArrayEncoding::SingleObject),
            17 => self.read_single_array(depth, ArrayEncoding::SingleString),
            21 | 22 => Err(NrbfError::Unsupported {
                offset: record_offset,
                feature: "method invocation records",
            }),
            _ => Err(NrbfError::UnsupportedRecord {
                offset: record_offset,
                record_type,
            }),
        }
    }

    fn read_class_definition(
        &mut self,
        record_type: u8,
        depth: usize,
    ) -> Result<RecordOutcome, NrbfError> {
        let id = self.read_i32()?;
        let name = self.read_string()?;
        let member_count = self.read_nonnegative_count("class member count")?;
        if member_count > self.limits.max_members {
            return Err(self.limit("class members", member_count, self.limits.max_members));
        }
        let mut member_names = Vec::new();
        self.reserve(&mut member_names, member_count)?;
        for _ in 0..member_count {
            member_names.push(self.read_string()?);
        }
        let member_types = if record_type == 4 || record_type == 5 {
            Some(self.read_member_types(member_count)?)
        } else {
            None
        };
        let library_id = if record_type == 3 || record_type == 5 {
            let id = self.read_i32()?;
            if !self.stream.libraries.contains_key(&id) {
                return Err(self.malformed("class library was not declared earlier"));
            }
            Some(id)
        } else {
            None
        };
        self.register_id(id)?;
        let schema = ClassSchema {
            name,
            member_names,
            member_types,
            library_id,
        };
        self.stream.schemas.insert(id, schema.clone());
        let class = self.read_class_body(id, id, &schema, depth)?;
        self.insert_object(id, NrbfObject::Class(class))?;
        Ok(RecordOutcome::Value(NrbfValue::Reference(id)))
    }

    fn read_class_with_id(&mut self, depth: usize) -> Result<RecordOutcome, NrbfError> {
        let object_id = self.read_i32()?;
        let schema_id = self.read_i32()?;
        let schema = self
            .stream
            .schemas
            .get(&schema_id)
            .cloned()
            .ok_or_else(|| self.malformed("class metadata id was not declared earlier"))?;
        self.register_id(object_id)?;
        let class = self.read_class_body(object_id, schema_id, &schema, depth)?;
        self.insert_object(object_id, NrbfObject::Class(class))?;
        Ok(RecordOutcome::Value(NrbfValue::Reference(object_id)))
    }

    fn read_class_body(
        &mut self,
        object_id: ObjectId,
        schema_id: ObjectId,
        schema: &ClassSchema,
        depth: usize,
    ) -> Result<NrbfClass, NrbfError> {
        let count = schema.member_names.len();
        let mut members = Vec::new();
        self.reserve(&mut members, count)?;
        while members.len() < count {
            let index = members.len();
            let primitive = schema
                .member_types
                .as_ref()
                .and_then(|types| match types.get(index) {
                    Some(BinaryType::Primitive(primitive)) => Some(*primitive),
                    _ => None,
                });
            if let Some(primitive) = primitive {
                let value = self.read_primitive(primitive)?;
                members.push(NrbfValue::Primitive(value));
                continue;
            }
            match self.read_record(depth + 1)? {
                RecordOutcome::Value(value) => members.push(value),
                RecordOutcome::Library => {}
                RecordOutcome::MessageEnd => {
                    return Err(self.malformed("message ended inside a class"));
                }
                RecordOutcome::Nulls(null_count) => {
                    self.append_nulls(
                        &mut members,
                        count,
                        null_count,
                        schema.member_types.as_ref(),
                    )?;
                }
            }
        }
        Ok(NrbfClass {
            object_id,
            schema_id,
            members,
        })
    }

    fn read_single_array(
        &mut self,
        depth: usize,
        encoding: ArrayEncoding,
    ) -> Result<RecordOutcome, NrbfError> {
        let id = self.read_i32()?;
        let length = self.read_nonnegative_count("array length")?;
        self.check_array_length(&[length])?;
        let element_type = match encoding {
            ArrayEncoding::SinglePrimitive => {
                let primitive = self.read_primitive_type()?;
                if matches!(primitive, PrimitiveType::Null | PrimitiveType::String) {
                    return Err(self.malformed("invalid single primitive array element type"));
                }
                BinaryType::Primitive(primitive)
            }
            ArrayEncoding::SingleObject => BinaryType::Object,
            ArrayEncoding::SingleString => BinaryType::String,
            ArrayEncoding::Binary(_) => return Err(self.malformed("invalid single-array encoding")),
        };
        self.register_id(id)?;
        let items = self.read_array_items(length, &element_type, depth)?;
        self.insert_object(
            id,
            NrbfObject::Array(NrbfArray {
                encoding,
                lengths: vec![length],
                lower_bounds: None,
                element_type,
                items,
            }),
        )?;
        Ok(RecordOutcome::Value(NrbfValue::Reference(id)))
    }

    fn read_binary_array(&mut self, depth: usize) -> Result<RecordOutcome, NrbfError> {
        let id = self.read_i32()?;
        let array_type = match self.read_u8()? {
            0 => BinaryArrayType::Single,
            1 => BinaryArrayType::Jagged,
            2 => BinaryArrayType::Rectangular,
            3 => BinaryArrayType::SingleOffset,
            4 => BinaryArrayType::JaggedOffset,
            5 => BinaryArrayType::RectangularOffset,
            _ => return Err(self.malformed("unknown binary array type")),
        };
        let rank = self.read_nonnegative_count("array rank")?;
        if rank > self.limits.max_array_rank {
            return Err(self.limit("array rank", rank, self.limits.max_array_rank));
        }
        let mut lengths = Vec::new();
        self.reserve(&mut lengths, rank)?;
        for _ in 0..rank {
            lengths.push(self.read_nonnegative_count("array dimension")?);
        }
        self.check_array_length(&lengths)?;
        let has_offsets = matches!(
            array_type,
            BinaryArrayType::SingleOffset
                | BinaryArrayType::JaggedOffset
                | BinaryArrayType::RectangularOffset
        );
        let lower_bounds = if has_offsets {
            let mut bounds = Vec::new();
            self.reserve(&mut bounds, rank)?;
            for _ in 0..rank {
                bounds.push(self.read_i32()?);
            }
            Some(bounds)
        } else {
            None
        };
        let element_type = self.read_binary_type()?;
        let length = lengths
            .iter()
            .try_fold(1usize, |product, length| product.checked_mul(*length));
        let length = length.ok_or_else(|| self.malformed("array element count overflow"))?;
        self.register_id(id)?;
        let items = self.read_array_items(length, &element_type, depth)?;
        self.insert_object(
            id,
            NrbfObject::Array(NrbfArray {
                encoding: ArrayEncoding::Binary(array_type),
                lengths,
                lower_bounds,
                element_type,
                items,
            }),
        )?;
        Ok(RecordOutcome::Value(NrbfValue::Reference(id)))
    }

    fn read_array_items(
        &mut self,
        length: usize,
        element_type: &BinaryType,
        depth: usize,
    ) -> Result<Vec<NrbfValue>, NrbfError> {
        let mut items = Vec::new();
        let storage = length
            .checked_mul(std::mem::size_of::<NrbfValue>())
            .ok_or_else(|| self.malformed("array storage size overflow"))?;
        if storage > self.limits.max_array_bytes {
            return Err(self.limit("array storage bytes", storage, self.limits.max_array_bytes));
        }
        self.reserve(&mut items, length)?;
        if let BinaryType::Primitive(primitive) = element_type {
            let fixed_size = primitive_fixed_size(*primitive);
            if let Some(size) = fixed_size {
                let payload = length
                    .checked_mul(size)
                    .ok_or_else(|| self.malformed("primitive array byte count overflow"))?;
                if payload > self.limits.max_array_bytes {
                    return Err(self.limit(
                        "array payload bytes",
                        payload,
                        self.limits.max_array_bytes,
                    ));
                }
            }
            for _ in 0..length {
                let value = self.read_primitive(*primitive)?;
                items.push(NrbfValue::Primitive(value));
            }
            return Ok(items);
        }

        while items.len() < length {
            match self.read_record(depth + 1)? {
                RecordOutcome::Value(value) => items.push(value),
                RecordOutcome::Library => {}
                RecordOutcome::MessageEnd => {
                    return Err(self.malformed("message ended inside an array"));
                }
                RecordOutcome::Nulls(count) => {
                    self.append_nulls(&mut items, length, count, None)?;
                }
            }
        }
        Ok(items)
    }

    fn append_nulls(
        &self,
        values: &mut Vec<NrbfValue>,
        expected: usize,
        count: usize,
        member_types: Option<&Vec<BinaryType>>,
    ) -> Result<(), NrbfError> {
        let new_length = values
            .len()
            .checked_add(count)
            .ok_or_else(|| self.malformed("null run length overflow"))?;
        if count == 0 || new_length > expected {
            return Err(self.malformed("null run exceeds member count"));
        }
        if let Some(types) = member_types {
            for index in values.len()..new_length {
                if matches!(types.get(index), Some(BinaryType::Primitive(_))) {
                    return Err(self.malformed("null record for a primitive class member"));
                }
            }
        }
        values.resize(new_length, NrbfValue::Null);
        Ok(())
    }

    fn read_member_types(&mut self, count: usize) -> Result<Vec<BinaryType>, NrbfError> {
        let mut kinds = Vec::new();
        self.reserve(&mut kinds, count)?;
        for _ in 0..count {
            kinds.push(self.read_binary_kind()?);
        }
        let mut types = Vec::new();
        self.reserve(&mut types, count)?;
        for kind in kinds {
            types.push(self.read_additional_type_info(kind)?);
        }
        Ok(types)
    }

    fn read_binary_type(&mut self) -> Result<BinaryType, NrbfError> {
        let kind = self.read_binary_kind()?;
        self.read_additional_type_info(kind)
    }

    fn read_binary_kind(&mut self) -> Result<BinaryKind, NrbfError> {
        match self.read_u8()? {
            0 => Ok(BinaryKind::Primitive),
            1 => Ok(BinaryKind::String),
            2 => Ok(BinaryKind::Object),
            3 => Ok(BinaryKind::SystemClass),
            4 => Ok(BinaryKind::Class),
            5 => Ok(BinaryKind::ObjectArray),
            6 => Ok(BinaryKind::StringArray),
            7 => Ok(BinaryKind::PrimitiveArray),
            _ => Err(self.malformed("unknown binary type")),
        }
    }

    fn read_additional_type_info(&mut self, kind: BinaryKind) -> Result<BinaryType, NrbfError> {
        match kind {
            BinaryKind::Primitive => {
                let primitive = self.read_primitive_type()?;
                if matches!(primitive, PrimitiveType::Null | PrimitiveType::String) {
                    return Err(self.malformed("invalid primitive class member type"));
                }
                Ok(BinaryType::Primitive(primitive))
            }
            BinaryKind::String => Ok(BinaryType::String),
            BinaryKind::Object => Ok(BinaryType::Object),
            BinaryKind::SystemClass => Ok(BinaryType::SystemClass(self.read_string()?)),
            BinaryKind::Class => {
                let name = self.read_string()?;
                let library_id = self.read_i32()?;
                if !self.stream.libraries.contains_key(&library_id) {
                    return Err(self.malformed("member class library was not declared earlier"));
                }
                Ok(BinaryType::Class { name, library_id })
            }
            BinaryKind::ObjectArray => Ok(BinaryType::ObjectArray),
            BinaryKind::StringArray => Ok(BinaryType::StringArray),
            BinaryKind::PrimitiveArray => {
                Ok(BinaryType::PrimitiveArray(self.read_primitive_type()?))
            }
        }
    }

    fn read_primitive_type(&mut self) -> Result<PrimitiveType, NrbfError> {
        match self.read_u8()? {
            1 => Ok(PrimitiveType::Boolean),
            2 => Ok(PrimitiveType::Byte),
            3 => Ok(PrimitiveType::Char),
            5 => Ok(PrimitiveType::Decimal),
            6 => Ok(PrimitiveType::Double),
            7 => Ok(PrimitiveType::Int16),
            8 => Ok(PrimitiveType::Int32),
            9 => Ok(PrimitiveType::Int64),
            10 => Ok(PrimitiveType::SByte),
            11 => Ok(PrimitiveType::Single),
            12 => Ok(PrimitiveType::TimeSpan),
            13 => Ok(PrimitiveType::DateTime),
            14 => Ok(PrimitiveType::UInt16),
            15 => Ok(PrimitiveType::UInt32),
            16 => Ok(PrimitiveType::UInt64),
            17 => Ok(PrimitiveType::Null),
            18 => Ok(PrimitiveType::String),
            _ => Err(self.malformed("unknown primitive type")),
        }
    }

    fn read_primitive(&mut self, primitive: PrimitiveType) -> Result<PrimitiveValue, NrbfError> {
        Ok(match primitive {
            PrimitiveType::Boolean => PrimitiveValue::Boolean(self.read_u8()? != 0),
            PrimitiveType::Byte => PrimitiveValue::Byte(self.read_u8()?),
            PrimitiveType::Char => PrimitiveValue::Char(self.read_char()?),
            PrimitiveType::Decimal => PrimitiveValue::Decimal(self.read_string()?),
            PrimitiveType::Double => PrimitiveValue::DoubleBits(self.read_u64()?),
            PrimitiveType::Int16 => PrimitiveValue::Int16(self.read_i16()?),
            PrimitiveType::Int32 => PrimitiveValue::Int32(self.read_i32()?),
            PrimitiveType::Int64 => PrimitiveValue::Int64(self.read_i64()?),
            PrimitiveType::SByte => PrimitiveValue::SByte(self.read_u8()? as i8),
            PrimitiveType::Single => PrimitiveValue::SingleBits(self.read_u32()?),
            PrimitiveType::TimeSpan => PrimitiveValue::TimeSpan(self.read_i64()?),
            PrimitiveType::DateTime => PrimitiveValue::DateTime(self.read_u64()?),
            PrimitiveType::UInt16 => PrimitiveValue::UInt16(self.read_u16()?),
            PrimitiveType::UInt32 => PrimitiveValue::UInt32(self.read_u32()?),
            PrimitiveType::UInt64 => PrimitiveValue::UInt64(self.read_u64()?),
            PrimitiveType::Null => PrimitiveValue::Null,
            PrimitiveType::String => PrimitiveValue::String(self.read_string()?),
        })
    }

    fn read_char(&mut self) -> Result<char, NrbfError> {
        let offset = self.position;
        let first = self.read_u8()?;
        let width = match first {
            0x00..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return Err(NrbfError::InvalidUtf8 { offset }),
        };
        let mut bytes = [0u8; 4];
        bytes[0] = first;
        for byte in bytes.iter_mut().take(width).skip(1) {
            *byte = self.read_u8()?;
        }
        let text =
            std::str::from_utf8(&bytes[..width]).map_err(|_| NrbfError::InvalidUtf8 { offset })?;
        let mut characters = text.chars();
        let character = characters.next().ok_or(NrbfError::InvalidUtf8 { offset })?;
        if characters.next().is_some() {
            return Err(NrbfError::InvalidUtf8 { offset });
        }
        Ok(character)
    }

    fn read_string(&mut self) -> Result<String, NrbfError> {
        let offset = self.position;
        let length = self.read_length()? as usize;
        if length > self.limits.max_string_bytes {
            return Err(self.limit("string bytes", length, self.limits.max_string_bytes));
        }
        let bytes = self.read_bytes(length)?;
        let text = std::str::from_utf8(bytes).map_err(|_| NrbfError::InvalidUtf8 { offset })?;
        Ok(text.to_owned())
    }

    fn read_length(&mut self) -> Result<u32, NrbfError> {
        let mut value = 0u32;
        for index in 0..5 {
            let byte = self.read_u8()?;
            if index == 4 && byte & 0xf8 != 0 {
                return Err(self.malformed("length prefix exceeds 31 bits"));
            }
            if index > 0 && byte == 0 {
                return Err(self.malformed("non-minimal length prefix"));
            }
            value |= u32::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(self.malformed("unterminated length prefix"))
    }

    fn read_nonnegative_count(&mut self, reason: &'static str) -> Result<usize, NrbfError> {
        let value = self.read_i32()?;
        if value < 0 {
            return Err(self.malformed(reason));
        }
        Ok(value as usize)
    }

    fn check_array_length(&self, lengths: &[usize]) -> Result<(), NrbfError> {
        let mut product = 1usize;
        for length in lengths {
            product = product
                .checked_mul(*length)
                .ok_or_else(|| self.malformed("array element count overflow"))?;
        }
        if product > self.limits.max_array_elements {
            return Err(self.limit("array elements", product, self.limits.max_array_elements));
        }
        Ok(())
    }

    fn register_id(&mut self, id: ObjectId) -> Result<(), NrbfError> {
        if !self.ids.insert(id) {
            return Err(self.malformed("duplicate object id"));
        }
        if self.ids.len() > self.limits.max_objects {
            return Err(self.limit("objects", self.ids.len(), self.limits.max_objects));
        }
        Ok(())
    }

    fn insert_object(&mut self, id: ObjectId, object: NrbfObject) -> Result<(), NrbfError> {
        if self.stream.objects.insert(id, object).is_some() {
            return Err(self.malformed("duplicate object id"));
        }
        Ok(())
    }

    fn validate_references(&self) -> Result<(), NrbfError> {
        if !self.stream.objects.contains_key(&self.stream.root_id) {
            return Err(self.malformed("root object is missing"));
        }
        for object in self.stream.objects.values() {
            match object {
                NrbfObject::Class(class) => {
                    let schema = self
                        .stream
                        .schemas
                        .get(&class.schema_id)
                        .ok_or_else(|| self.malformed("class schema is missing"))?;
                    if class.members.len() != schema.member_names.len() {
                        return Err(self.malformed("class member count mismatch"));
                    }
                    for value in &class.members {
                        self.validate_value(value)?;
                    }
                }
                NrbfObject::Array(array) => {
                    for value in &array.items {
                        self.validate_value(value)?;
                    }
                }
                NrbfObject::String(_) => {}
            }
        }
        Ok(())
    }

    fn validate_value(&self, value: &NrbfValue) -> Result<(), NrbfError> {
        if let NrbfValue::Reference(id) = value
            && (*id <= 0 || !self.stream.objects.contains_key(id))
        {
            return Err(self.malformed("dangling object reference"));
        }
        Ok(())
    }

    fn reserve<T>(&self, values: &mut Vec<T>, count: usize) -> Result<(), NrbfError> {
        values
            .try_reserve_exact(count)
            .map_err(|_| NrbfError::Allocation {
                offset: self.position,
                elements: count,
            })
    }

    fn bump_records(&mut self) -> Result<(), NrbfError> {
        self.records = self.records.saturating_add(1);
        if self.records > self.limits.max_records {
            return Err(self.limit("records", self.records, self.limits.max_records));
        }
        Ok(())
    }

    fn read_bytes(&mut self, length: usize) -> Result<&'a [u8], NrbfError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| self.malformed("input offset overflow"))?;
        if end > self.readable {
            if self.input.len() > self.readable {
                return Err(self.limit("NRBF stream bytes", end, self.limits.max_stream_bytes));
            }
            return Err(NrbfError::UnexpectedEof {
                offset: self.position,
                needed: length,
                remaining: self.readable.saturating_sub(self.position),
            });
        }
        let bytes = &self.input[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn read_u8(&mut self) -> Result<u8, NrbfError> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, NrbfError> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    fn read_i16(&mut self) -> Result<i16, NrbfError> {
        Ok(i16::from_le_bytes(self.read_array()?))
    }

    fn read_u32(&mut self) -> Result<u32, NrbfError> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    fn read_i32(&mut self) -> Result<i32, NrbfError> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    fn read_u64(&mut self) -> Result<u64, NrbfError> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }

    fn read_i64(&mut self) -> Result<i64, NrbfError> {
        Ok(i64::from_le_bytes(self.read_array()?))
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], NrbfError> {
        let mut bytes = [0u8; N];
        bytes.copy_from_slice(self.read_bytes(N)?);
        Ok(bytes)
    }

    fn malformed(&self, reason: &'static str) -> NrbfError {
        NrbfError::Malformed {
            offset: self.position,
            reason,
        }
    }

    fn limit(&self, limit: &'static str, requested: usize, maximum: usize) -> NrbfError {
        NrbfError::ResourceLimit {
            offset: self.position,
            limit,
            requested: requested as u64,
            maximum,
        }
    }
}

fn primitive_fixed_size(primitive: PrimitiveType) -> Option<usize> {
    match primitive {
        PrimitiveType::Boolean | PrimitiveType::Byte | PrimitiveType::SByte => Some(1),
        PrimitiveType::Int16 | PrimitiveType::UInt16 => Some(2),
        PrimitiveType::Int32 | PrimitiveType::UInt32 | PrimitiveType::Single => Some(4),
        PrimitiveType::Double
        | PrimitiveType::Int64
        | PrimitiveType::TimeSpan
        | PrimitiveType::DateTime
        | PrimitiveType::UInt64 => Some(8),
        PrimitiveType::Null => Some(0),
        PrimitiveType::Char | PrimitiveType::Decimal | PrimitiveType::String => None,
    }
}
