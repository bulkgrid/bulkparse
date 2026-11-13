//! Repair legal PDF comment/whitespace separators without touching string data.
//! The underlying content parser accepts only a subset of PDF whitespace and
//! can silently return a parsed prefix. Always check complete consumption.
use lopdf::content::Content;

pub(super) fn decode(bytes: &[u8]) -> (Content, bool, bool) {
    if let Ok(content) = Content::decode_strict(bytes) {
        return (content, false, false);
    }
    let fixed = separators(bytes);
    if let Ok(content) = Content::decode_strict(&fixed) {
        return (content, true, false);
    }
    // Retain available native content, but the caller must mark this page for
    // review rather than claiming that an empty prefix means a blank page.
    let content = Content::decode(&fixed).unwrap_or(Content { operations: vec![] });
    (content, false, true)
}

fn separators(bytes: &[u8]) -> Vec<u8> {
    let mut fixed = bytes.to_vec();
    let mut i = 0;
    let mut inline = false;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                while i < bytes.len() && !matches!(bytes[i], b'\n' | b'\r') {
                    fixed[i] = b' ';
                    i += 1;
                }
            }
            b'\0' | b'\x0c' => {
                fixed[i] = b' ';
                i += 1;
            }
            b'(' => {
                let mut depth = 1usize;
                i += 1;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'\\' => i += 1,
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
            }
            b'<' if bytes.get(i + 1) != Some(&b'<') => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'>' {
                    i += 1;
                }
                i += usize::from(i < bytes.len());
            }
            b'<' if bytes.get(i + 1) == Some(&b'<') => i += 2,
            b'/' => {
                i += 1;
                while i < bytes.len() && !delimiter(bytes[i]) {
                    i += 1;
                }
            }
            b if !delimiter(b) => {
                let start = i;
                while i < bytes.len() && !delimiter(bytes[i]) {
                    i += 1;
                }
                let token = &bytes[start..i];
                if inline && token == b"ID" {
                    // Binary inline images can contain arbitrary percent signs,
                    // parentheses and whitespace. Leave this suffix untouched;
                    // strict decoding will expose any unresolved later syntax.
                    break;
                }
                inline |= token == b"BI";
            }
            _ => i += 1,
        }
    }
    fixed
}

fn delimiter(b: u8) -> bool {
    b.is_ascii_whitespace() || b == 0 || b"()<>[]{}/%".contains(&b)
}

#[cfg(test)]
mod tests {
    use super::separators;
    #[test]
    fn lexical_repair_preserves_strings_hex_and_inline_binary() {
        assert_eq!(
            separators(b"% comment\n(50% \\(raw\\)) Tj <2528> Tj\0\x0cQ"),
            b"         \n(50% \\(raw\\)) Tj <2528> Tj  Q"
        );
        assert_eq!(
            separators(b"% comment\nBI /W 2 /H 1 ID %\0(\x0c EI Q"),
            b"         \nBI /W 2 /H 1 ID %\0(\x0c EI Q"
        );
    }
}
