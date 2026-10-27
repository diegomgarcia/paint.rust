use super::*;

pub(super) fn write(stream: &NrbfStream, limits: &NrbfLimits) -> Result<Vec<u8>, NrbfError> {
    Writer {
        output: Vec::new(),
        limits,
        emitted_objects: BTreeSet::new(),
        emitted_schemas: BTreeSet::new(),
    }
    .write_stream(stream)
}

struct Writer<'a> {
    output: Vec<u8>,
    limits: &'a NrbfLimits,
    emitted_objects: BTreeSet<ObjectId>,
    emitted_schemas: BTreeSet<ObjectId>,
}

impl Writer<'_> {
    fn write_stream(mut self, stream: &NrbfStream) -> Result<Vec<u8>, NrbfError> {
        if stream.root_id <= 0 || !stream.objects.contains_key(&stream.root_id) {
            return Err(self.malformed("root object is missing or has a non-positive id"));
        }
        self.write_u8(0)?;
        self.write_i32(stream.root_id)?;
        self.write_i32(stream.header_id)?;
        self.write_i32(1)?;
        self.write_i32(0)?;
        for (id, name) in &stream.libraries {
            self.write_u8(12)?;
            self.write_i32(*id)?;
            self.write_string(name)?;
        }
        self.write_object(stream, stream.root_id, 0)?;
        let remaining = stream.objects.keys().copied().collect::<Vec<_>>();
        for id in remaining {
            if !self.emitted_objects.contains(&id) {
                self.write_object(stream, id, 0)?;
            }
        }
        self.write_u8(11)?;
        Ok(self.output)
    }

    fn write_object(
        &mut self,
        stream: &NrbfStream,
        id: ObjectId,
        depth: usize,
    ) -> Result<(), NrbfError> {
        if depth > self.limits.max_depth {
            return Err(self.limit("object depth", depth, self.limits.max_depth));
        }
        if self.emitted_objects.contains(&id) {
            if id <= 0 {
                return Err(self.malformed("cannot write a member reference to a non-positive id"));
            }
            self.write_u8(9)?;
            return self.write_i32(id);
        }
        let object = stream
            .objects
            .get(&id)
            .cloned()
            .ok_or_else(|| self.malformed("dangling object reference"))?;
        match object {
            NrbfObject::String(value) => {
                self.emitted_objects.insert(id);
                self.write_u8(6)?;
                self.write_i32(id)?;
                self.write_string(&value)?;
            }
            NrbfObject::Array(array) => {
                self.emitted_objects.insert(id);
                self.write_array(stream, id, &array, depth)?;
            }
            NrbfObject::Class(class) => {
                if class.object_id != id || class.schema_id != id {
                    if !self.emitted_schemas.contains(&class.schema_id) {
                        self.write_object(stream, class.schema_id, depth + 1)?;
                    }
                    if self.emitted_objects.contains(&id) {
                        self.write_u8(9)?;
                        return self.write_i32(id);
                    }
                }
                let schema = stream
                    .schemas
                    .get(&class.schema_id)
                    .cloned()
                    .ok_or_else(|| self.malformed("class schema is missing"))?;
                if class.members.len() != schema.member_names.len() {
                    return Err(self.malformed("class member count mismatch"));
                }
                if schema
                    .library_id
                    .is_some_and(|library| !stream.libraries.contains_key(&library))
                {
                    return Err(self.malformed("class library is missing"));
                }
                self.emitted_objects.insert(id);
                if self.emitted_schemas.contains(&class.schema_id) {
                    self.write_u8(1)?;
                    self.write_i32(id)?;
                    self.write_i32(class.schema_id)?;
                } else {
                    if id != class.schema_id {
                        return Err(self.malformed("first class instance does not own its schema"));
                    }
                    match (schema.library_id, schema.member_types.is_some()) {
                        (Some(_), true) => self.write_u8(5)?,
                        (Some(_), false) => self.write_u8(3)?,
                        (None, true) => self.write_u8(4)?,
                        (None, false) => self.write_u8(2)?,
                    }
                    self.write_class_info(id, &schema)?;
                    if let Some(types) = &schema.member_types {
                        self.write_member_types(types)?;
                    }
                    if let Some(library_id) = schema.library_id {
                        self.write_i32(library_id)?;
                    }
                    self.emitted_schemas.insert(class.schema_id);
                }
                for (index, value) in class.members.iter().enumerate() {
                    let primitive =
                        schema
                            .member_types
                            .as_ref()
                            .and_then(|types| match types.get(index) {
                                Some(BinaryType::Primitive(primitive)) => Some(*primitive),
                                _ => None,
                            });
                    if let Some(primitive) = primitive {
                        let NrbfValue::Primitive(value) = value else {
                            return Err(self.malformed("primitive class member has a record value"));
                        };
                        self.write_primitive(primitive, value)?;
                    } else {
                        self.write_record_value(stream, value, depth + 1)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn write_class_info(&mut self, id: ObjectId, schema: &ClassSchema) -> Result<(), NrbfError> {
        self.write_i32(id)?;
        self.write_string(&schema.name)?;
        self.write_count(schema.member_names.len(), "class members")?;
        for name in &schema.member_names {
            self.write_string(name)?;
        }
        Ok(())
    }

    fn write_member_types(&mut self, types: &[BinaryType]) -> Result<(), NrbfError> {
        for binary_type in types {
            self.write_u8(binary_kind(binary_type))?;
        }
        for binary_type in types {
            self.write_additional_type_info(binary_type)?;
        }
        Ok(())
    }

    fn write_additional_type_info(&mut self, binary_type: &BinaryType) -> Result<(), NrbfError> {
        match binary_type {
            BinaryType::Primitive(primitive) | BinaryType::PrimitiveArray(primitive) => {
                self.write_u8(*primitive as u8)?;
            }
            BinaryType::SystemClass(name) => self.write_string(name)?,
            BinaryType::Class { name, library_id } => {
                self.write_string(name)?;
                self.write_i32(*library_id)?;
            }
            BinaryType::String
            | BinaryType::Object
            | BinaryType::ObjectArray
            | BinaryType::StringArray => {}
        }
        Ok(())
    }

    fn write_array(
        &mut self,
        stream: &NrbfStream,
        id: ObjectId,
        array: &NrbfArray,
        depth: usize,
    ) -> Result<(), NrbfError> {
        let expected = array
            .lengths
            .iter()
            .try_fold(1usize, |product, length| product.checked_mul(*length));
        if expected != Some(array.items.len()) {
            return Err(self.malformed("array dimensions do not match item count"));
        }
        match array.encoding {
            ArrayEncoding::SinglePrimitive => {
                let BinaryType::Primitive(primitive) = array.element_type else {
                    return Err(self.malformed("single primitive array has an object type"));
                };
                if matches!(primitive, PrimitiveType::Null | PrimitiveType::String) {
                    return Err(self.malformed("invalid single primitive array element type"));
                }
                self.require_single_dimension(array)?;
                self.write_u8(15)?;
                self.write_i32(id)?;
                self.write_count(array.items.len(), "array elements")?;
                self.write_u8(primitive as u8)?;
                self.write_primitive_items(primitive, &array.items)?;
            }
            ArrayEncoding::SingleObject | ArrayEncoding::SingleString => {
                self.require_single_dimension(array)?;
                self.write_u8(if array.encoding == ArrayEncoding::SingleObject {
                    16
                } else {
                    17
                })?;
                self.write_i32(id)?;
                self.write_count(array.items.len(), "array elements")?;
                for value in &array.items {
                    self.write_record_value(stream, value, depth + 1)?;
                }
            }
            ArrayEncoding::Binary(array_type) => {
                self.write_u8(7)?;
                self.write_i32(id)?;
                self.write_u8(array_type as u8)?;
                self.write_count(array.lengths.len(), "array rank")?;
                for length in &array.lengths {
                    self.write_count(*length, "array dimension")?;
                }
                let has_offsets = matches!(
                    array_type,
                    BinaryArrayType::SingleOffset
                        | BinaryArrayType::JaggedOffset
                        | BinaryArrayType::RectangularOffset
                );
                match (&array.lower_bounds, has_offsets) {
                    (Some(bounds), true) if bounds.len() == array.lengths.len() => {
                        for bound in bounds {
                            self.write_i32(*bound)?;
                        }
                    }
                    (None, false) => {}
                    _ => return Err(self.malformed("array lower bounds do not match its encoding")),
                }
                self.write_u8(binary_kind(&array.element_type))?;
                self.write_additional_type_info(&array.element_type)?;
                if let BinaryType::Primitive(primitive) = array.element_type {
                    self.write_primitive_items(primitive, &array.items)?;
                } else {
                    for value in &array.items {
                        self.write_record_value(stream, value, depth + 1)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn require_single_dimension(&self, array: &NrbfArray) -> Result<(), NrbfError> {
        if array.lengths.len() != 1 || array.lower_bounds.is_some() {
            return Err(self.malformed("single array has non-single dimensions"));
        }
        Ok(())
    }

    fn write_primitive_items(
        &mut self,
        primitive: PrimitiveType,
        items: &[NrbfValue],
    ) -> Result<(), NrbfError> {
        for item in items {
            let NrbfValue::Primitive(value) = item else {
                return Err(self.malformed("primitive array contains a record value"));
            };
            self.write_primitive(primitive, value)?;
        }
        Ok(())
    }

    fn write_record_value(
        &mut self,
        stream: &NrbfStream,
        value: &NrbfValue,
        depth: usize,
    ) -> Result<(), NrbfError> {
        match value {
            NrbfValue::Null => self.write_u8(10),
            NrbfValue::Reference(id) => self.write_object(stream, *id, depth),
            NrbfValue::Primitive(value) => {
                if primitive_type(value) == PrimitiveType::String {
                    return Err(self.malformed("typed primitive record cannot contain a string"));
                }
                self.write_u8(8)?;
                self.write_u8(primitive_type(value) as u8)?;
                self.write_primitive(primitive_type(value), value)
            }
        }
    }

    fn write_primitive(
        &mut self,
        expected: PrimitiveType,
        value: &PrimitiveValue,
    ) -> Result<(), NrbfError> {
        if primitive_type(value) != expected {
            return Err(self.malformed("primitive value does not match its declared type"));
        }
        match value {
            PrimitiveValue::Boolean(value) => self.write_u8(u8::from(*value)),
            PrimitiveValue::Byte(value) => self.write_u8(*value),
            PrimitiveValue::Char(value) => {
                let mut buffer = [0u8; 4];
                let text = value.encode_utf8(&mut buffer);
                self.write_bytes(text.as_bytes())
            }
            PrimitiveValue::Decimal(value) => self.write_string(value),
            PrimitiveValue::DoubleBits(value) => self.write_u64(*value),
            PrimitiveValue::Int16(value) => self.write_i16(*value),
            PrimitiveValue::Int32(value) => self.write_i32(*value),
            PrimitiveValue::Int64(value) => self.write_i64(*value),
            PrimitiveValue::SByte(value) => self.write_u8(*value as u8),
            PrimitiveValue::SingleBits(value) => self.write_u32(*value),
            PrimitiveValue::TimeSpan(value) => self.write_i64(*value),
            PrimitiveValue::DateTime(value) => self.write_u64(*value),
            PrimitiveValue::UInt16(value) => self.write_u16(*value),
            PrimitiveValue::UInt32(value) => self.write_u32(*value),
            PrimitiveValue::UInt64(value) => self.write_u64(*value),
            PrimitiveValue::Null => Ok(()),
            PrimitiveValue::String(value) => self.write_string(value),
        }
    }

    fn write_string(&mut self, value: &str) -> Result<(), NrbfError> {
        if value.len() > self.limits.max_string_bytes {
            return Err(self.limit("string bytes", value.len(), self.limits.max_string_bytes));
        }
        let length = u32::try_from(value.len())
            .map_err(|_| self.limit("string bytes", value.len(), i32::MAX as usize))?;
        let mut offset = 0;
        loop {
            let mut byte = ((length >> offset) & 0x7f) as u8;
            offset += 7;
            if offset < 32 && length >> offset != 0 {
                byte |= 0x80;
            }
            self.write_u8(byte)?;
            if byte & 0x80 == 0 {
                break;
            }
        }
        self.write_bytes(value.as_bytes())
    }

    fn write_count(&mut self, count: usize, limit: &'static str) -> Result<(), NrbfError> {
        let count =
            i32::try_from(count).map_err(|_| self.limit(limit, count, i32::MAX as usize))?;
        self.write_i32(count)
    }

    fn write_u8(&mut self, value: u8) -> Result<(), NrbfError> {
        self.write_bytes(&[value])
    }

    fn write_u16(&mut self, value: u16) -> Result<(), NrbfError> {
        self.write_bytes(&value.to_le_bytes())
    }

    fn write_i16(&mut self, value: i16) -> Result<(), NrbfError> {
        self.write_bytes(&value.to_le_bytes())
    }

    fn write_u32(&mut self, value: u32) -> Result<(), NrbfError> {
        self.write_bytes(&value.to_le_bytes())
    }

    fn write_i32(&mut self, value: i32) -> Result<(), NrbfError> {
        self.write_bytes(&value.to_le_bytes())
    }

    fn write_u64(&mut self, value: u64) -> Result<(), NrbfError> {
        self.write_bytes(&value.to_le_bytes())
    }

    fn write_i64(&mut self, value: i64) -> Result<(), NrbfError> {
        self.write_bytes(&value.to_le_bytes())
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), NrbfError> {
        let end = self
            .output
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| self.malformed("output length overflow"))?;
        if end > self.limits.max_output_bytes {
            return Err(self.limit("NRBF output bytes", end, self.limits.max_output_bytes));
        }
        self.output
            .try_reserve_exact(bytes.len())
            .map_err(|_| NrbfError::Allocation {
                offset: self.output.len(),
                elements: bytes.len(),
            })?;
        self.output.extend_from_slice(bytes);
        Ok(())
    }

    fn malformed(&self, reason: &'static str) -> NrbfError {
        NrbfError::Malformed {
            offset: self.output.len(),
            reason,
        }
    }

    fn limit(&self, limit: &'static str, requested: usize, maximum: usize) -> NrbfError {
        NrbfError::ResourceLimit {
            offset: self.output.len(),
            limit,
            requested: requested as u64,
            maximum,
        }
    }
}

fn binary_kind(binary_type: &BinaryType) -> u8 {
    match binary_type {
        BinaryType::Primitive(_) => 0,
        BinaryType::String => 1,
        BinaryType::Object => 2,
        BinaryType::SystemClass(_) => 3,
        BinaryType::Class { .. } => 4,
        BinaryType::ObjectArray => 5,
        BinaryType::StringArray => 6,
        BinaryType::PrimitiveArray(_) => 7,
    }
}

fn primitive_type(value: &PrimitiveValue) -> PrimitiveType {
    match value {
        PrimitiveValue::Boolean(_) => PrimitiveType::Boolean,
        PrimitiveValue::Byte(_) => PrimitiveType::Byte,
        PrimitiveValue::Char(_) => PrimitiveType::Char,
        PrimitiveValue::Decimal(_) => PrimitiveType::Decimal,
        PrimitiveValue::DoubleBits(_) => PrimitiveType::Double,
        PrimitiveValue::Int16(_) => PrimitiveType::Int16,
        PrimitiveValue::Int32(_) => PrimitiveType::Int32,
        PrimitiveValue::Int64(_) => PrimitiveType::Int64,
        PrimitiveValue::SByte(_) => PrimitiveType::SByte,
        PrimitiveValue::SingleBits(_) => PrimitiveType::Single,
        PrimitiveValue::TimeSpan(_) => PrimitiveType::TimeSpan,
        PrimitiveValue::DateTime(_) => PrimitiveType::DateTime,
        PrimitiveValue::UInt16(_) => PrimitiveType::UInt16,
        PrimitiveValue::UInt32(_) => PrimitiveType::UInt32,
        PrimitiveValue::UInt64(_) => PrimitiveType::UInt64,
        PrimitiveValue::Null => PrimitiveType::Null,
        PrimitiveValue::String(_) => PrimitiveType::String,
    }
}
