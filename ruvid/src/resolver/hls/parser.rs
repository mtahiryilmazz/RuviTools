use std::collections::BTreeMap;

/// Parses HLS attribute lists like `BANDWIDTH=1280000,RESOLUTION=1280x720,CODECS="avc1.4d401f,mp4a.40.2"`.
pub fn parse_attribute_list(raw: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut chars = raw.trim().char_indices().peekable();

    while let Some(&(_, _)) = chars.peek() {
        // Skip commas/spaces
        while let Some(&(_, c)) = chars.peek() {
            if c == ',' || c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        // Key
        let mut key = String::new();
        while let Some(&(_, c)) = chars.peek() {
            if c == '=' {
                chars.next();
                break;
            }
            key.push(c);
            chars.next();
        }
        let key = key.trim().to_ascii_uppercase();
        if key.is_empty() {
            break;
        }
        // Value
        let mut val = String::new();
        if let Some(&(_, '"')) = chars.peek() {
            chars.next();
            for (_, c) in chars.by_ref() {
                if c == '"' {
                    break;
                }
                val.push(c);
            }
        } else {
            while let Some(&(_, c)) = chars.peek() {
                if c == ',' {
                    chars.next();
                    break;
                }
                val.push(c);
                chars.next();
            }
        }
        map.insert(key, val.trim().to_string());
    }
    map
}

pub fn parse_resolution(raw: &str) -> (Option<u32>, Option<u32>) {
    if let Some((w, h)) = raw.trim().split_once('x') {
        let width = w.parse::<u32>().ok();
        let height = h.parse::<u32>().ok();
        (width, height)
    } else {
        (None, None)
    }
}
