use serde::Serialize;

// Both API validation and the frontend limits endpoint use these values.
pub const MAX_PAYLOAD_TERMS: usize = 10;
pub const MAX_PAYLOAD_TERM_CHARS: usize = 128;

#[derive(Serialize)]
pub struct PayloadSearchLimits {
    pub max_terms: usize,
    pub max_term_chars: usize,
}

pub fn limits() -> PayloadSearchLimits {
    PayloadSearchLimits {
        max_terms: MAX_PAYLOAD_TERMS,
        max_term_chars: MAX_PAYLOAD_TERM_CHARS,
    }
}

#[derive(Default)]
pub struct PayloadSearch {
    terms: Vec<Vec<u8>>,
}

impl PayloadSearch {
    pub fn new(terms: Vec<String>) -> Result<Self, String> {
        if terms.len() > MAX_PAYLOAD_TERMS {
            return Err(format!(
                "payload must contain at most {MAX_PAYLOAD_TERMS} terms"
            ));
        }
        let terms = terms
            .into_iter()
            .map(|term| {
                // Match JavaScript String.trim(), including BOM and excluding NEL.
                let term = term.trim_matches(|c| {
                    matches!(c,
                        '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
                        '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' |
                        '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
                    )
                });
                if term.is_empty() {
                    return Err("payload terms must not be blank".into());
                }
                if term.chars().count() > MAX_PAYLOAD_TERM_CHARS {
                    return Err(format!(
                        "payload terms must not exceed {MAX_PAYLOAD_TERM_CHARS} characters"
                    ));
                }
                Ok(term.as_bytes().to_vec())
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { terms })
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn matches(&self, c2s: &[u8], s2c: &[u8]) -> bool {
        if self.is_empty() {
            return true;
        }
        let c2s = crate::payload_text::display(c2s, false);
        let s2c = crate::payload_text::display(s2c, false);
        self.terms
            .iter()
            .all(|term| contains(c2s.as_bytes(), term) || contains(s2c.as_bytes(), term))
    }
}

fn contains(bytes: &[u8], term: &[u8]) -> bool {
    bytes.windows(term.len()).any(|window| window == term)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn and_matches_whole_session_without_joining_directions() {
        let search = PayloadSearch::new(vec!["POST".into(), "FLAG".into(), "POST".into()]).unwrap();
        assert!(search.matches(b"header POST body", b"reply FLAG"));
        assert!(!search.matches(b"post body", b"reply FLAG"));
        assert!(!search.matches(b"POST", b"no flag"));
        let boundary = PayloadSearch::new(vec!["AB".into()]).unwrap();
        assert!(!boundary.matches(b"A", b"B"));
        let literal = PayloadSearch::new(vec!["A OR B.*".into()]).unwrap();
        assert!(literal.matches(b"A OR B.*", b""));
        assert!(!literal.matches(b"A", b"B"));
        assert!(PayloadSearch::default().matches(b"", b""));
    }

    #[test]
    fn validates_duplicates_and_unicode_after_browser_compatible_trimming() {
        assert!(PayloadSearch::new(vec!["same".into(); MAX_PAYLOAD_TERMS]).is_ok());
        assert!(PayloadSearch::new(vec!["same".into(); MAX_PAYLOAD_TERMS + 1]).is_err());
        assert!(PayloadSearch::new(vec![format!(
            "\u{feff} {} ",
            "🦀".repeat(MAX_PAYLOAD_TERM_CHARS)
        )])
        .is_ok());
        assert!(PayloadSearch::new(vec!["🦀".repeat(MAX_PAYLOAD_TERM_CHARS + 1)]).is_err());
        assert!(PayloadSearch::new(vec![" \n\u{feff}".into()]).is_err());
        assert!(PayloadSearch::new(vec!["\u{0085}".into()])
            .unwrap()
            .matches("\u{0085}".as_bytes(), b""));
    }

    #[test]
    fn matches_decoded_unicode_nested_json_and_percent_encoding() {
        let search = PayloadSearch::new(vec!["Привет".into(), "A".into()]).unwrap();
        assert!(search.matches(
            br#"{"nested":"{\"text\":\"\u041f\u0440\u0438\u0432\u0435\u0442\"}"}"#,
            b"%41"
        ));
        assert!(!search.matches(
            br#"{"text":"\u041f\u0440\u0438\u0432\u0435\u0442"}"#,
            b"%61"
        ));
        assert!(!PayloadSearch::new(vec!["AB".into()])
            .unwrap()
            .matches(b"%41", br"\u0042"));
        assert!(!PayloadSearch::new(vec!["%41".into()])
            .unwrap()
            .matches(b"%41", b""));
    }
}
