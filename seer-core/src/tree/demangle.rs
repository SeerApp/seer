use rustc_demangle::demangle as rustc_demangle;

fn strip_hash_suffix(name: &str) -> &str {
    if let Some(idx) = name.rfind("::h") {
        let suffix = &name[idx + 3..];
        if suffix.len() == 16 && suffix.chars().all(|c| c.is_ascii_hexdigit()) {
            return &name[..idx];
        }
    }
    name
}

pub fn demangle(raw: &String) -> String {
    let clean = strip_hash_suffix(&rustc_demangle(raw).to_string()).to_string();
    return clean;
}
