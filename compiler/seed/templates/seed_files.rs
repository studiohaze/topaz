use std::io::Read;
use std::path::Path;

pub fn read_bounded_utf8(path: &Path, limit: usize) -> Result<String, String> {
    let count = limit.checked_add(1).ok_or("invalid file byte limit")?;
    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(count as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!("{} exceeds the {limit}-byte limit", path.display()));
    }
    String::from_utf8(bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))
}
