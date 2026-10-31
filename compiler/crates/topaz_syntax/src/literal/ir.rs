//! Literal codec identity belongs to the producing wire row, not its spelling.
use super::decode_string_escapes;

pub const CHECKED_LITERAL_ENCODING: &str = "checked-values/v1";
pub const LEGACY_LITERAL_ENCODING: &str = "legacy-source/v1";
/// Explicit literal-table extension marker before the operation count.
pub const LITERAL_TABLE_MARKER: u32 = u32::MAX;
pub const LITERAL_TABLE_ENCODING: &str = "topaz.literal-table/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrLiteralEncoding {
    LegacySource,
    CheckedValues,
}
impl IrLiteralEncoding {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            LEGACY_LITERAL_ENCODING => Ok(Self::LegacySource),
            CHECKED_LITERAL_ENCODING => Ok(Self::CheckedValues),
            _ => Err(format!("unknown literal encoding `{value}`")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::LegacySource => LEGACY_LITERAL_ENCODING,
            Self::CheckedValues => CHECKED_LITERAL_ENCODING,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrLiteral {
    Integer(i64),
    Float(u64),
    Text(String),
    Duration(u64),
}

/// Called once at program admission. Failures stay attached to the original op/span.
pub fn decode_ir_literal(
    kind: &str,
    detail: &str,
    encoding: IrLiteralEncoding,
) -> Option<Result<IrLiteral, String>> {
    Some(match kind {
        "expression/integer" => detail
            .parse::<i64>()
            .map(IrLiteral::Integer)
            .map_err(|_| format!("invalid Stage 1 integer `{detail}`")),
        "expression/float" => match encoding {
            IrLiteralEncoding::LegacySource => detail
                .parse::<f64>()
                .map(|v| IrLiteral::Float(v.to_bits()))
                .map_err(|_| format!("invalid Stage 1 float `{detail}`")),
            IrLiteralEncoding::CheckedValues => {
                if detail.len() != 16 || !detail.bytes().all(|b| b.is_ascii_hexdigit()) {
                    Err("invalid checked float bits".into())
                } else {
                    u64::from_str_radix(detail, 16)
                        .map(IrLiteral::Float)
                        .map_err(|_| "invalid checked float bits".into())
                }
            }
        },
        "expression/string-text" => match encoding {
            IrLiteralEncoding::LegacySource => {
                let mut out = String::new();
                decode_string_escapes(detail, &mut out).map(|()| IrLiteral::Text(out))
            }
            IrLiteralEncoding::CheckedValues => Ok(IrLiteral::Text(detail.to_string())),
        },
        "expression/duration" => match encoding {
            IrLiteralEncoding::LegacySource => [
                ("ms", crate::DurationUnit::Ms),
                ("s", crate::DurationUnit::S),
                ("m", crate::DurationUnit::M),
            ]
            .into_iter()
            .find_map(|(suffix, unit)| {
                let magnitude = detail.strip_suffix(suffix)?;
                if magnitude.is_empty() || !magnitude.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                magnitude
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| unit.checked_milliseconds(n))
            })
            .map(IrLiteral::Duration)
            .ok_or_else(|| "invalid Stage 1 duration".into()),
            IrLiteralEncoding::CheckedValues => detail
                .parse::<u64>()
                .map(IrLiteral::Duration)
                .map_err(|_| "invalid checked duration milliseconds".into()),
        },
        _ => return None,
    })
}
