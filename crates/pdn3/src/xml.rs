use quick_xml::Reader;
use quick_xml::events::Event;

use crate::{PdnError, PdnLimits};

#[derive(Debug)]
pub(crate) struct RootXml {
    pub width: u32,
    pub height: u32,
    pub layers: usize,
    pub saved_with: String,
}

pub(crate) fn parse_header(xml: &str, limits: &PdnLimits) -> Result<RootXml, PdnError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().check_comments = true;
    let mut depth = 0usize;
    let mut elements = 0usize;
    let mut root = None;
    let mut root_seen = false;
    loop {
        match reader
            .read_event()
            .map_err(|error| PdnError::Xml(error.to_string()))?
        {
            Event::Start(element) => {
                elements = count_element(elements, limits)?;
                if depth == 0 {
                    if root_seen || element.name().as_ref() != b"pdnImage" {
                        return Err(malformed_xml("PDN XML root must be one pdnImage element"));
                    }
                    root_seen = true;
                    root = Some(parse_root_attributes(&reader, &element)?);
                }
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| malformed_xml("XML depth overflow"))?;
                if depth > limits.max_xml_depth {
                    return Err(PdnError::ResourceLimit {
                        resource: "XML depth",
                        requested: depth as u64,
                        maximum: limits.max_xml_depth,
                    });
                }
            }
            Event::Empty(element) => {
                elements = count_element(elements, limits)?;
                if depth == 0 {
                    if root_seen || element.name().as_ref() != b"pdnImage" {
                        return Err(malformed_xml("PDN XML root must be one pdnImage element"));
                    }
                    root_seen = true;
                    root = Some(parse_root_attributes(&reader, &element)?);
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| malformed_xml("unbalanced XML end element"))?;
            }
            Event::DocType(_) => {
                return Err(malformed_xml(
                    "DTD and entity declarations are not supported",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 {
        return Err(malformed_xml("unclosed XML element"));
    }
    root.ok_or_else(|| malformed_xml("missing pdnImage root"))
}

fn count_element(elements: usize, limits: &PdnLimits) -> Result<usize, PdnError> {
    let count = elements
        .checked_add(1)
        .ok_or_else(|| malformed_xml("XML element overflow"))?;
    if count > limits.max_xml_elements {
        return Err(PdnError::ResourceLimit {
            resource: "XML elements",
            requested: count as u64,
            maximum: limits.max_xml_elements,
        });
    }
    Ok(count)
}

fn parse_root_attributes(
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
) -> Result<RootXml, PdnError> {
    let mut width = None;
    let mut height = None;
    let mut layers = None;
    let mut saved_with = None;
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| PdnError::Xml(error.to_string()))?;
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .map_err(|error| PdnError::Xml(error.to_string()))?
            .into_owned();
        match attribute.key.as_ref() {
            b"width" => width = Some(parse_number(&value, "invalid XML width")?),
            b"height" => height = Some(parse_number(&value, "invalid XML height")?),
            b"layers" => layers = Some(parse_number(&value, "invalid XML layer count")?),
            b"savedWithVersion" => saved_with = Some(value),
            _ => {}
        }
    }
    Ok(RootXml {
        width: width.ok_or_else(|| malformed_xml("missing XML width"))?,
        height: height.ok_or_else(|| malformed_xml("missing XML height"))?,
        layers: layers.ok_or_else(|| malformed_xml("missing XML layer count"))?,
        saved_with: saved_with.ok_or_else(|| malformed_xml("missing savedWithVersion"))?,
    })
}

fn parse_number<T: std::str::FromStr>(value: &str, reason: &'static str) -> Result<T, PdnError> {
    value.parse().map_err(|_| malformed_xml(reason))
}

fn malformed_xml(reason: &'static str) -> PdnError {
    PdnError::Malformed { offset: 0, reason }
}
