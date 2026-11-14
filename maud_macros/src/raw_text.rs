//! Classification of `<script>` and `<style>` bodies, and the check for text
//! that would end them early.

// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
// !!!!!!!!!! PLEASE KEEP THESE IN SYNC WITH `maud/src/ctx.rs` !!!!!!!!!!!!!!!!!
// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

/// Sequences that may not appear in a `<script>` body, lowercase. See
/// `SCRIPT_FORBIDDEN` in `maud/src/ctx.rs` for why `<!--` is not on the list.
pub const SCRIPT_FORBIDDEN: &[&str] = &["</script", "<script"];

/// Sequences that may not appear in a `<style>` body, lowercase.
pub const STYLE_FORBIDDEN: &[&str] = &["</style"];

/// Finds every occurrence of `needles` (lowercase ASCII) in `haystack`,
/// ignoring ASCII case. Returns `(needle, byte offset)` pairs.
pub fn find_forbidden(haystack: &str, needles: &[&'static str]) -> Vec<(&'static str, usize)> {
    let lower = haystack.to_ascii_lowercase();
    let mut hits = Vec::new();
    for start in 0..lower.len() {
        for needle in needles {
            if lower.as_bytes()[start..].starts_with(needle.as_bytes()) {
                hits.push((*needle, start));
            }
        }
    }
    hits
}

/// How the body of a `<script>` element is treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    /// Run by the browser, or not understood by us.
    Executable,
    /// `application/json` or `application/ld+json`.
    Json,
    /// `text/template` or `text/x-template`.
    Template,
}

impl ScriptKind {
    /// Classifies a static `type` attribute value.
    ///
    /// The browser strips ASCII whitespace and ignores case when it decides
    /// whether to run a script, and it runs several spellings (`module`, the
    /// empty string, a dozen JavaScript MIME types). So anything other than the
    /// four data types below counts as executable, which only ever makes the
    /// rules stricter.
    pub fn from_type(value: &str) -> Self {
        let value = value
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .to_ascii_lowercase();
        match value.as_str() {
            "application/json" | "application/ld+json" => Self::Json,
            "text/template" | "text/x-template" => Self::Template,
            _ => Self::Executable,
        }
    }
}

/// Elements in which a `script` or `style` is rejected: in `svg` and `math`
/// it becomes a foreign element whose character references are decoded, and in
/// the others it is plain text, so its literals could close the parent.
pub const UNSUPPORTED_PARENTS: &[&str] = &[
    "svg",
    "math",
    "textarea",
    "title",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "plaintext",
    "frameset",
];

/// Whether `c` cannot appear in an attribute name: the HTML tokenizer ends the
/// name there (or, for `"`, `'` and `<`, reports a parse error), so a string
/// literal name containing it would turn into other attributes. NUL becomes
/// U+FFFD.
pub fn is_invalid_in_attribute_name(c: char) -> bool {
    c.is_ascii_whitespace() || matches!(c, '/' | '=' | '>' | '"' | '\'' | '<' | '\0')
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn classify() {
        for (value, kind) in [
            ("application/json", ScriptKind::Json),
            (" Application/LD+JSON\n", ScriptKind::Json),
            ("text/x-template", ScriptKind::Template),
            ("", ScriptKind::Executable),
            ("module", ScriptKind::Executable),
            (" text/javascript ", ScriptKind::Executable),
            ("TEXT/JAVASCRIPT", ScriptKind::Executable),
            ("importmap", ScriptKind::Executable),
            ("speculationrules", ScriptKind::Executable),
            ("application/json; charset=utf-8", ScriptKind::Executable),
            // U+00A0 is not ASCII whitespace, so the browser keeps it.
            ("\u{a0}application/json", ScriptKind::Executable),
        ] {
            assert_eq!(ScriptKind::from_type(value), kind, "{value:?}");
        }
    }

    #[test]
    fn forbidden() {
        assert_eq!(
            find_forbidden("a</ScRiPt>b<SCRIPT", SCRIPT_FORBIDDEN),
            vec![("</script", 1), ("<script", 11)],
        );
        assert!(find_forbidden("if (a<!--b) {}", SCRIPT_FORBIDDEN).is_empty());
        assert_eq!(
            find_forbidden("</STYLE", STYLE_FORBIDDEN),
            vec![("</style", 0)]
        );
    }
}
