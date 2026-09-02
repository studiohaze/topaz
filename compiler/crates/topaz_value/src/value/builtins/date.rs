use super::super::*;

pub(in crate::value) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(in crate::value) const DATE_MIN_DAYS: i64 = -719_528;
pub(in crate::value) const DATE_MAX_DAYS: i64 = 2_932_896;

pub(in crate::value) fn date_in_supported_range(days: i64) -> bool {
    (DATE_MIN_DAYS..=DATE_MAX_DAYS).contains(&days)
}

pub(in crate::value) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

pub(in crate::value) fn is_valid_ymd(year: i64, month: i64, day: i64) -> bool {
    if !(0..=9999).contains(&year) {
        return false;
    }
    if !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    let days = days_from_civil(year, month, day);
    civil_from_days(days) == (year, month, day)
}

pub(in crate::value) fn date_int_arg(
    arg: Value,
    owner: &str,
    name: &str,
    param: &str,
    span: Span,
) -> Result<i64, RtError> {
    match arg {
        Value::Int(n) => Ok(n),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`{owner}.{name}` takes `{param}: int`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn date_arg(arg: Value, name: &str, span: Span) -> Result<DateData, RtError> {
    match arg {
        Value::Date(d) => Ok(d),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`Date.{name}` takes `Date`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub(in crate::value) fn parse_iso_date(text: &str) -> Result<DateData, String> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err("Date.parseIso: expected YYYY-MM-DD".to_string());
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return Err("Date.parseIso: expected ASCII digits in YYYY-MM-DD".to_string());
    }
    let decimal = |digits: &[u8]| {
        digits
            .iter()
            .fold(0_i64, |value, digit| value * 10 + i64::from(digit - b'0'))
    };
    let year = decimal(&bytes[0..4]);
    let month = decimal(&bytes[5..7]);
    let day = decimal(&bytes[8..10]);
    if !is_valid_ymd(year, month, day) {
        return Err(format!("Date.parseIso: invalid date `{text}`"));
    }
    Ok(DateData {
        days: days_from_civil(year, month, day),
    })
}

pub(in crate::value) fn date_to_iso(date: DateData) -> String {
    debug_assert!(date_in_supported_range(date.days));
    let (year, month, day) = civil_from_days(date.days);
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn builtin_date_from_ymd(
    year: Value,
    month: Value,
    day: Value,
    span: Span,
) -> Result<Value, RtError> {
    let year = date_int_arg(year, "Date", "fromYmd", "year", span)?;
    let month = date_int_arg(month, "Date", "fromYmd", "month", span)?;
    let day = date_int_arg(day, "Date", "fromYmd", "day", span)?;
    Ok(if is_valid_ymd(year, month, day) {
        Value::Ok(Rc::new(Value::Date(DateData {
            days: days_from_civil(year, month, day),
        })))
    } else {
        Value::Err(Rc::new(Value::Str(Rc::from(format!(
            "Date.fromYmd: invalid date {year:04}-{month:02}-{day:02}"
        )))))
    })
}

pub fn builtin_date_parse_iso(text: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "Date", "parseIso", "text", span)?;
    Ok(match parse_iso_date(&text) {
        Ok(date) => Value::Ok(Rc::new(Value::Date(date))),
        Err(e) => Value::Err(Rc::new(Value::Str(Rc::from(e)))),
    })
}

pub fn builtin_date_to_iso(recv: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Str(Rc::from(date_to_iso(date_arg(
        recv, "toIso", span,
    )?))))
}

pub fn builtin_date_add_days(recv: Value, days: Value, span: Span) -> Result<Value, RtError> {
    let date = date_arg(recv, "addDays", span)?;
    let days = date_int_arg(days, "Date", "addDays", "days", span)?;
    let Some(next) = date.days.checked_add(days) else {
        return Err(fault(
            codes::FAULT_OVERFLOW,
            "Date.addDays overflowed the supported date range",
            span,
        ));
    };
    if !date_in_supported_range(next) {
        return Err(fault(
            codes::FAULT_OVERFLOW,
            "Date.addDays overflowed the supported date range",
            span,
        ));
    }
    Ok(Value::Date(DateData { days: next }))
}

pub fn builtin_date_year(recv: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Int(
        civil_from_days(date_arg(recv, "year", span)?.days).0,
    ))
}

pub fn builtin_date_month(recv: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Int(
        civil_from_days(date_arg(recv, "month", span)?.days).1,
    ))
}

pub fn builtin_date_day(recv: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Int(
        civil_from_days(date_arg(recv, "day", span)?.days).2,
    ))
}
