//! Renders `script` and `style` elements, parses the output with a real HTML
//! parser, and checks that the document has the intended shape.

use html5ever::{ParseOpts, local_name, ns, parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use maud::{Markup, PreEscaped, TrustedScript, html, trusted_script};
use std::panic::{UnwindSafe, catch_unwind};

/// Parses a page and lists its elements in document order, skipping the
/// implied `html`, `head` and `body`. Each entry is the element name (prefixed
/// with `svg:` or `math:` for foreign elements), its attributes, and its
/// direct text content.
fn summarize(markup: Markup) -> Vec<String> {
    let html = markup.into_string();
    let dom = parse_document(RcDom::default(), ParseOpts::default()).one(html.as_str());
    let mut out = Vec::new();
    walk(&dom.document, &mut out);
    out
}

fn walk(node: &Handle, out: &mut Vec<String>) {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        let implied = name.ns == ns!(html)
            && (name.local == local_name!("html")
                || name.local == local_name!("head")
                || name.local == local_name!("body"));
        if !implied {
            let prefix = if name.ns == ns!(svg) {
                "svg:"
            } else if name.ns == ns!(mathml) {
                "math:"
            } else {
                ""
            };
            let mut entry = format!("{prefix}{}", name.local);
            for attr in attrs.borrow().iter() {
                entry.push_str(&format!("[{}={}]", attr.name.local, attr.value));
            }
            let text = text_of(node);
            if !text.is_empty() {
                entry.push_str(&format!("{{{text}}}"));
            }
            out.push(entry);
        }
    }
    for child in node.children.borrow().iter() {
        walk(child, out);
    }
}

fn text_of(node: &Handle) -> String {
    let mut text = String::new();
    for child in node.children.borrow().iter() {
        if let NodeData::Text { contents } = &child.data {
            text.push_str(&contents.borrow());
        }
    }
    text
}

/// Returns the text of the only `script` element in a page.
fn script_text(markup: Markup) -> String {
    let html = markup.into_string();
    let dom = parse_document(RcDom::default(), ParseOpts::default()).one(html.as_str());
    let mut scripts = Vec::new();
    find_scripts(&dom.document, &mut scripts);
    assert_eq!(scripts.len(), 1, "{html}");
    text_of(&scripts[0])
}

fn find_scripts(node: &Handle, out: &mut Vec<Handle>) {
    if let NodeData::Element { name, .. } = &node.data
        && name.local == local_name!("script")
    {
        out.push(node.clone());
    }
    for child in node.children.borrow().iter() {
        find_scripts(child, out);
    }
}

fn assert_panics(f: impl FnOnce() + UnwindSafe) {
    assert!(catch_unwind(f).is_err(), "expected a panic");
}

#[test]
fn executable_script_round_trips() {
    let code = "if (a < b && c > d) { go('<b>&amp;'); }";
    assert_eq!(
        summarize(html! { script { "if (a < b && c > d) { go('<b>&amp;'); }" } p { "end" } }),
        [format!("script{{{code}}}"), "p{end}".into()],
    );
}

#[test]
fn types_that_run_are_executable() {
    // Each of these only compiles because the `type` is classified as
    // executable: a JSON block would not accept a `TrustedScript`.
    let code = trusted_script!("a<b && c>d");
    let dynamic_type = "application/json";
    let optional_type = Some("application/json");
    for (markup, ty) in [
        (
            html! { script type="TEXT/JAVASCRIPT" { (code) } p { "end" } },
            "TEXT/JAVASCRIPT",
        ),
        (
            html! { script type=" text/javascript " { (code) } p { "end" } },
            " text/javascript ",
        ),
        (
            html! { script type="module" { (code) } p { "end" } },
            "module",
        ),
        (html! { script type="" { (code) } p { "end" } }, ""),
        (
            html! { script type="importmap" { (code) } p { "end" } },
            "importmap",
        ),
        (
            html! { script type="speculationrules" { (code) } p { "end" } },
            "speculationrules",
        ),
        (
            html! { script type=(dynamic_type) { (code) } p { "end" } },
            dynamic_type,
        ),
        (
            html! { script type=[optional_type] { (code) } p { "end" } },
            dynamic_type,
        ),
        (
            html! { script type="application/json" type="text/javascript" { (code) } p { "end" } },
            "application/json",
        ),
    ] {
        assert_eq!(
            summarize(markup),
            [format!("script[type={ty}]{{a<b && c>d}}"), "p{end}".into()],
        );
    }
}

#[test]
fn style_round_trips() {
    assert_eq!(
        summarize(html! { style { "a > b::after { content: '<&>' }" } p { "end" } }),
        ["style{a > b::after { content: '<&>' }}", "p{end}"],
    );
}

#[test]
fn comment_open_alone_does_not_hide_the_end_tag() {
    assert_eq!(
        summarize(html! { script { "<!-- x\nif (a<!--b) {}\n-->" } p { "end" } }),
        ["script{<!-- x\nif (a<!--b) {}\n-->}", "p{end}"],
    );
}

#[test]
fn attribute_injection_on_script() {
    let src = "\"><img src=x onerror=alert(1)>";
    assert_eq!(
        summarize(html! { script src=(src) {} p { "end" } }),
        [format!("script[src={src}]"), "p{end}".into()],
    );
}

#[test]
fn breakouts_through_trusted_code_panic() {
    assert_panics(|| {
        let _ = html! { script { (TrustedScript::assume_trusted("<!--<script>")) } };
    });
    assert_panics(|| {
        let _ = html! { script { (TrustedScript::assume_trusted("</ScRiPt ><img src=x>")) } };
    });
    assert_panics(|| {
        let _ = html! { script { "</scr" (trusted_script!("ipt>")) } };
    });
    assert_panics(|| {
        let _ = html! { script { "<!--<scr" @if true { "ipt>" } @else { "" } } };
    });
    assert_panics(|| {
        let _ = html! { script { "<" @match 1 { 1 => "/script", _ => "" } ">" } };
    });
}

#[test]
fn template_script_breakout_panics() {
    assert_panics(|| {
        let _ = html! {
            script type="text/template" { (PreEscaped("</script><img src=x onerror=alert(1)>")) }
        };
    });
}

#[test]
fn template_script_text_is_entity_encoded() {
    // The browser does not decode character references in any `<script>`, so a
    // template reads back the escaped text, not the markup that was written.
    assert_eq!(
        script_text(html! { script type="text/template" { "<b>{{name}}</b>" } }),
        "&lt;b&gt;{{name}}&lt;/b&gt;",
    );
}

/// A `script` built in another `html!` call can still end up inside `svg` or
/// `math`, where the browser makes it a foreign element and decodes character
/// references. Each row stays safe there.
#[test]
fn rows_inside_foreign_content() {
    // Executable: only trusted code, so the worst case is the developer's own
    // `<` being parsed as markup.
    let code = trusted_script!("go();");
    let script = html! { script { (code) } };
    assert_eq!(
        summarize(html! { svg { (script) } p { "end" } }),
        ["svg:svg", "svg:script{go();}", "p{end}"],
    );

    // Template: escaped text has no `<`, and decoding the entities gives back
    // exactly the text that was spliced. No element is injected.
    let user = "<img src=x onerror=alert(1)>&amp;";
    let template = html! { script type="text/template" { (user) } };
    for wrapper in [
        html! { svg { (template.clone()) } p { "end" } },
        html! { math { (template.clone()) } p { "end" } },
    ] {
        let summary = summarize(wrapper);
        assert_eq!(summary.len(), 3, "{summary:?}");
        assert!(summary[1].ends_with(&format!("{{{user}}}")), "{summary:?}");
        assert_eq!(summary[2], "p{end}");
    }
}

#[cfg(feature = "json")]
mod json {
    use super::*;
    use maud::json::Json;
    use serde_json::{Value, json, value::RawValue};

    const NASTY: &[&str] = &[
        "</script><img src=x onerror=alert(1)>",
        "</ScRiPt >",
        "<!--<script>",
        "<!--",
        "-->",
        "\u{2028}\u{2029}",
        "\"'`",
        "\\u003c/script>",
        "&lt;/script&gt;",
        "\0\r\n",
    ];

    fn assert_json_round_trips(value: &Value, markup: Markup) {
        let summary = summarize(markup);
        assert_eq!(summary.len(), 2, "{summary:?}");
        assert!(summary[0].starts_with("script[type="), "{summary:?}");
        assert_eq!(summary[1], "p{end}");
        let text = summary[0]
            .split_once('{')
            .unwrap()
            .1
            .strip_suffix('}')
            .unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(&parsed, value);
    }

    #[test]
    fn raw_value_breakout() {
        let raw =
            RawValue::from_string(r#""</ScRiPt><img src=x onerror=alert(1)>""#.into()).unwrap();
        let value: Value = serde_json::from_str(raw.get()).unwrap();
        assert_json_round_trips(
            &value,
            html! { script type="application/ld+json" { (Json(&raw)) } p { "end" } },
        );
        assert_json_round_trips(
            &value,
            html! { script type="application/ld+json" { (&*raw) } p { "end" } },
        );
    }

    #[test]
    fn nasty_strings_in_keys_and_values() {
        for nasty in NASTY {
            let value = json!({ *nasty: [*nasty, { "k": *nasty }] });
            assert_json_round_trips(
                &value,
                html! { script type="application/json" { (Json(&value)) } p { "end" } },
            );
            assert_json_round_trips(
                &value,
                html! { script type=" Application/JSON " { (value) } p { "end" } },
            );
            let raw = RawValue::from_string(value.to_string()).unwrap();
            assert_json_round_trips(
                &value,
                html! { script type="application/ld+json" { (&*raw) } p { "end" } },
            );
        }
    }

    #[test]
    fn json_literal_and_splice_cannot_join_into_end_tag() {
        // `true` completes `</scrip` into `</script`.
        assert_panics(|| {
            let _ = html! { script type="application/json" { "</scrip" (Json(true)) } };
        });
    }

    #[test]
    fn json_inside_foreign_content() {
        // JSON output has no `<`, `>` or `&`, so entity decoding in foreign
        // content leaves it unchanged.
        for nasty in NASTY {
            let value = json!({ *nasty: *nasty });
            let block = html! { script type="application/json" { (Json(&value)) } };
            let summary = summarize(html! { svg { (block.clone()) } p { "end" } });
            assert_eq!(summary.len(), 3, "{summary:?}");
            assert_eq!(summary[2], "p{end}");
            let text = summary[1]
                .split_once('{')
                .unwrap()
                .1
                .strip_suffix('}')
                .unwrap();
            assert_eq!(serde_json::from_str::<Value>(text).unwrap(), value);
        }
    }

    #[test]
    fn json_docs_pattern() {
        let user = json!({ "name": "</script><!--" });
        let summary = summarize(html! {
            script type="application/json" id="user" { (Json(&user)) }
            script { "const user = JSON.parse(document.getElementById('user').textContent);" }
            p { "end" }
        });
        assert_eq!(summary.len(), 3, "{summary:?}");
        assert_eq!(
            summary[1],
            "script{const user = JSON.parse(document.getElementById('user').textContent);}"
        );
    }
}

// ---------------------------------------------------------------------------
// Attacks on the context-aware escaping. Each test asserts the secure
// outcome, so a failing test is a finding.
// ---------------------------------------------------------------------------

mod attacks {
    use super::*;

    /// Attacker-controlled text with no characters that HTML escaping touches.
    const PAYLOAD: &str = "alert(document.domain)";

    // F1, F2, F2b, F4, F6, F6c and F7 put a `script` inside `svg` or `math`
    // in the same `html!` call. They are compile errors now, both because of
    // the `svg`/`math` guard and because a `String` cannot be spliced into an
    // executable script: see `tests/warnings/raw-text-foreign.rs`.

    /// F3. A spliced `Markup` with a breakout element inside `svg`. Whatever
    /// the browser does with the `svg`, the script came from its own `html!`
    /// call, where it could only take trusted code.
    #[test]
    fn spliced_breakout_cannot_carry_data_into_a_script() {
        let para = html! { p { "hi" } };
        let script = html! { script { (trusted_script!("go();")) } };
        let page = html! { svg { (para) (script) } p { "end" } }.into_string();
        assert!(!page.contains(PAYLOAD));
        let summary = summarize(PreEscaped(page));
        assert_eq!(summary.last().unwrap(), "p{end}");
        assert!(summary.iter().any(|e| e == "script{go();}"), "{summary:?}");
    }

    /// F5 (pre-existing, not new in this change). A `Markup` spliced into an
    /// attribute value is pushed raw, so its own quotes end the attribute.
    #[test]
    #[ignore = "pre-existing; fixed separately by lambda-fairy/maud#510"]
    fn markup_in_attribute_injects_attributes() {
        let user = "x onmouseover=alert(1)//";
        let inner = html! { b title=(user) {} };
        let page = summarize(html! { div title=(inner) {} });
        assert!(
            !page.iter().any(|e| e.contains("[onmouseover=")),
            "{page:?}"
        );
    }

    /// F8. The caller's constant is called `CODE`, like the constant the macro
    /// used to define internally.
    #[test]
    fn trusted_script_accepts_a_const_named_code() {
        const CODE: &str = "go();";
        let style = {
            const CODE: &str = "p {}";
            maud::trusted_style!(CODE)
        };
        assert_eq!(
            html! { script { (trusted_script!(CODE)) } style { (style) } }.into_string(),
            "<script>go();</script><style>p {}</style>",
        );
    }
}

/// Attacks that held.
mod held {
    use super::*;

    #[test]
    fn split_and_escape_state_sequences_panic() {
        let t = |s: &'static str| TrustedScript::assume_trusted(s);
        for (a, b) in [
            ("<!-- <scr", "ipt>"),
            ("</SCR", "IPT\t"),
            ("<", "/script/"),
            ("<!--<sCrIpT", " -->"),
        ] {
            assert_panics(move || {
                let _ = html! { script { (t(a)) (t(b)) } };
            });
            assert_panics(move || {
                let _ = html! { script { @for x in [t(a), t(b)] { (x) } } };
            });
        }
        assert_panics(|| {
            let _ = html! { style { (maud::TrustedStyle::assume_trusted("</StYlE ")) } };
        });
        assert_panics(|| {
            let _ = html! { script type="text/x-template" { (PreEscaped("<!--<script>")) } };
        });
    }

    #[cfg(feature = "json")]
    #[test]
    fn raw_value_nested_and_boxed() {
        use serde_json::value::RawValue;
        let raw: Box<RawValue> =
            RawValue::from_string(r#"{"</script>":"<!--<script> &"}"#.into()).unwrap();
        let map = std::collections::BTreeMap::from([("k</script>", &*raw)]);
        let html = html! {
            script type="application/json" { (raw) }
            script type=" APPLICATION/LD+JSON " { (maud::json::Json(&map)) }
            p { "end" }
        }
        .into_string();
        assert!(!html.contains("</script>\""), "{html}");
        assert_eq!(summarize(PreEscaped(html)).last().unwrap(), "p{end}");
    }
}

// ---------------------------------------------------------------------------
// attack3: tokenizer and tree-builder states, parent content models, serde
// paths, sealing, and template instantiation. Module `a3_findings` holds
// tests that assert the secure outcome and FAIL (each is a finding); module
// `a3_held` holds attacks that did not break anything.
// ---------------------------------------------------------------------------

mod a3 {
    use super::*;
    use html5ever::{QualName, parse_fragment, tree_builder::TreeBuilderOpts};

    pub const PAYLOAD: &str = "alert(document.domain)";

    pub struct El {
        pub ns: &'static str,
        pub name: String,
        pub attrs: Vec<(String, String)>,
        pub text: String,
    }

    fn opts(scripting: bool) -> ParseOpts {
        ParseOpts {
            tree_builder: TreeBuilderOpts {
                scripting_enabled: scripting,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn elements(html: &str, scripting: bool) -> Vec<El> {
        let dom = parse_document(RcDom::default(), opts(scripting)).one(html);
        let mut out = Vec::new();
        collect(&dom.document, &mut out);
        out
    }

    /// What `container.innerHTML = html` builds, for a `div` container.
    pub fn fragment_elements(html: &str) -> Vec<El> {
        let dom = parse_fragment(
            RcDom::default(),
            opts(true),
            QualName::new(None, ns!(html), local_name!("div")),
            Vec::new(),
            true,
        )
        .one(html);
        let mut out = Vec::new();
        collect(&dom.document, &mut out);
        out
    }

    pub fn collect_public(node: &Handle, out: &mut Vec<El>) {
        collect(node, out);
    }

    fn collect(node: &Handle, out: &mut Vec<El>) {
        if let NodeData::Element {
            name,
            attrs,
            template_contents,
            ..
        } = &node.data
        {
            out.push(El {
                ns: if name.ns == ns!(svg) {
                    "svg"
                } else if name.ns == ns!(mathml) {
                    "math"
                } else {
                    "html"
                },
                name: name.local.to_string(),
                attrs: attrs
                    .borrow()
                    .iter()
                    .map(|a| (a.name.local.to_string(), a.value.to_string()))
                    .collect(),
                text: text_of(node),
            });
            if let Some(contents) = template_contents.borrow().as_ref() {
                collect(contents, out);
            }
        }
        for child in node.children.borrow().iter() {
            collect(child, out);
        }
    }

    /// Whether a browser runs a `script` with this `type`.
    pub fn runs(el: &El) -> bool {
        let ty = el.attrs.iter().find(|(n, _)| n == "type").map(|(_, v)| {
            v.trim_matches(|c: char| c.is_ascii_whitespace())
                .to_ascii_lowercase()
        });
        !matches!(
            ty.as_deref(),
            Some("application/json" | "application/ld+json" | "text/template" | "text/x-template")
        )
    }

    /// Ways in which attacker data took effect in a parsed element list.
    pub fn violations_in(els: &[El], out: &mut Vec<String>) {
        for el in els {
            if el.name == "img" {
                out.push(format!("injected <img> {:?}", el.attrs));
            }
            for (name, value) in &el.attrs {
                if name.starts_with("on") {
                    out.push(format!("injected handler {name}={value} on <{}>", el.name));
                }
                if name == "srcdoc" {
                    for scripting in [true, false] {
                        violations_in(&elements(value, scripting), out);
                    }
                }
            }
            if el.name == "script" && runs(el) && el.text.contains(PAYLOAD) {
                out.push(format!(
                    "{}:script {:?} runs attacker text {:?}",
                    el.ns, el.attrs, el.text
                ));
            }
            if el.name == "style" && el.text.contains(PAYLOAD) {
                out.push(format!("style holds attacker text {:?}", el.text));
            }
        }
    }

    pub fn violations(html: &str) -> Vec<String> {
        let mut out = Vec::new();
        for scripting in [true, false] {
            violations_in(&elements(html, scripting), &mut out);
        }
        out
    }

    /// Simulates a client-side framework that instantiates every template
    /// script: `div.innerHTML = script.textContent`.
    pub fn instantiation_violations(html: &str) -> Vec<String> {
        let mut out = Vec::new();
        for el in elements(html, true) {
            let is_template = el
                .attrs
                .iter()
                .any(|(n, v)| n == "type" && (v == "text/template" || v == "text/x-template"));
            if el.name == "script" && is_template {
                violations_in(&fragment_elements(&el.text), &mut out);
            }
        }
        out
    }

    pub const USER_INPUTS: &[&str] = &[
        PAYLOAD,
        "</script><img src=x onerror=alert(document.domain)>",
        "--></script><script>alert(document.domain)</script>",
        "<!--<script>",
        "</textarea></title></xmp></noscript></iframe></noembed></noframes></template></select></table></style><img src=x onerror=alert(document.domain)>",
        "&lt;img src=x onerror=alert(document.domain)&gt;",
        "&#60;/script&#62;&#60;img src=x onerror=alert(document.domain)&#62;",
        "\" onload=\"alert(document.domain)",
        "]]><img src=x onerror=alert(document.domain)>",
        "\u{0}</script\u{0}><img src=x onerror=alert(document.domain)>",
        "\r</script\r><img src=x onerror=alert(document.domain)>",
        "\u{feff}</script><img src=x onerror=alert(document.domain)>",
        "{{constructor.constructor('alert(document.domain)')()}}",
    ];

    /// One wrapper per parent element: `name { (inner) }`.
    macro_rules! wrappers {
        ($($name:ident)*) => {
            &[$((stringify!($name), |inner: Markup| html! { $name { (inner) } }),)*]
        };
    }

    pub type Wrapper = (&'static str, fn(Markup) -> Markup);

    pub const WRAPPERS: &[Wrapper] = wrappers! {
        div template noscript iframe textarea title xmp plaintext noembed noframes
        select option optgroup table tbody thead tr td th caption colgroup frameset
        head body html button a p pre listing object marquee details summary dialog
        form ul li dl dd h1 nobr b font ruby rt video audio canvas map label fieldset
        legend datalist applet svg math
    };

    pub const EXTRA_WRAPPERS: &[Wrapper] = &[
        (
            "svg>foreignObject",
            |i| html! { svg { foreignObject { (i) } } },
        ),
        ("svg>title", |i| html! { svg { title { (i) } } }),
        ("svg>desc", |i| html! { svg { desc { (i) } } }),
        ("math>mi", |i| html! { math { mi { (i) } } }),
        ("math>annotation-xml[html]", |i| {
            html! { math { annotation-xml encoding="text/html" { (i) } } }
        }),
        (
            "math>annotation-xml",
            |i| html! { math { annotation-xml { (i) } } },
        ),
        ("head>noscript", |i| html! { head { noscript { (i) } } }),
        (
            "html>head+frameset",
            |i| html! { html { head {} frameset { (i) } } },
        ),
        (
            "frameset>noframes",
            |i| html! { html { head {} frameset { noframes { (i) } } } },
        ),
        (
            "iframe[srcdoc]",
            |i| html! { iframe srcdoc=(i.into_string()) {} },
        ),
        ("iframe[srcdoc]x2", |i| {
            let inner = html! { iframe srcdoc=(i.into_string()) {} };
            html! { iframe srcdoc=(inner.into_string()) {} }
        }),
        ("unclosed textarea", |i| html! { textarea; (i) }),
        ("unclosed title", |i| html! { title; (i) }),
        ("unclosed xmp", |i| html! { xmp; (i) }),
        ("unclosed iframe", |i| html! { iframe; (i) }),
        ("unclosed noscript", |i| html! { noscript; (i) }),
        ("unclosed plaintext", |i| html! { plaintext; (i) }),
        ("after comment open", |i| html! { (PreEscaped("<!--")) (i) }),
        (
            "after cdata open",
            |i| html! { (PreEscaped("<![CDATA[")) (i) },
        ),
        (
            "svg after cdata open",
            |i| html! { svg { (PreEscaped("<![CDATA[")) (i) } },
        ),
        ("after doctype", |i| html! { (maud::DOCTYPE) (i) }),
        (
            "table>text",
            |i| html! { table { "x" (i) "y" tr { td { (i.clone()) } } } },
        ),
        ("select>table", |i| html! { select { table { (i) } } }),
        (
            "table>select",
            |i| html! { table { tr { td { select { (i) } } } } },
        ),
        (
            "template>frameset",
            |i| html! { template { frameset { (i) } } },
        ),
        ("template>tr", |i| html! { template { tr { (i) } } }),
        ("p>table", |i| html! { p { table { (i) } } }),
        ("a>a", |i| html! { a { b { a { (i) } } } }),
        ("form>form", |i| html! { form { form { (i) } } }),
        ("button>button", |i| html! { button { button { (i) } } }),
    ];

    pub fn all_wrappers() -> Vec<Wrapper> {
        WRAPPERS.iter().chain(EXTRA_WRAPPERS).copied().collect()
    }

    /// A tiny deterministic generator, to avoid a dependency.
    pub struct Rng(pub u64);
    impl Rng {
        pub fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        pub fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// The text a WHATWG tokenizer reports for raw text `s`.
    pub fn normalized(s: &str) -> String {
        s.replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\0', "\u{fffd}")
    }
}

mod a3_held {
    use super::{a3::*, *};
    use maud::TrustedStyle;

    const TOKENS: &[&str] = &[
        "<",
        "</",
        "<!--",
        "-->",
        "--",
        "-",
        "!",
        "/",
        ">",
        "script",
        "SCRIPT",
        "sCrIpT",
        "scr",
        "ipt",
        "style",
        "STYLE",
        " ",
        "\t",
        "\n",
        "\r",
        "\r\n",
        "\x0c",
        "\0",
        "\u{feff}",
        "x",
        "<script",
        "</script",
        "</style",
        "<style",
        "&lt;",
        "&#60;",
        "&#x3c;/script&#x3e;",
        "\u{17f}cript",
        "\u{212a}",
        "]]>",
        "<![CDATA[",
        "<?",
        "<%",
        "'",
        "\"",
        "`",
        "\\",
        "=",
        "\u{2028}",
        "\u{b}",
        "\u{a0}",
        "\u{85}",
    ];

    /// For every body the run-time check lets through, the parser must see one
    /// `script` (or `style`) whose text is exactly that body, followed by the
    /// sibling `p`. Covers the script data escaped and double-escaped states,
    /// every terminator after `</script`, CR/LF, NUL and BOM.
    fn check_body(body: &str, accepted: &mut usize) {
        let b = body.to_string();
        let script = catch_unwind(move || {
            html! { script { (TrustedScript::assume_trusted(b)) } p { "end" } }.into_string()
        });
        let b = body.to_string();
        let split = body.len() / 2;
        let split = (0..=split)
            .rev()
            .find(|i| body.is_char_boundary(*i))
            .unwrap();
        let (left, right) = (body[..split].to_string(), body[split..].to_string());
        let script_split = catch_unwind(move || {
            html! {
                script {
                    (TrustedScript::assume_trusted(left))
                    @if true { (TrustedScript::assume_trusted(right)) }
                }
                p { "end" }
            }
            .into_string()
        });
        assert_eq!(script.is_ok(), script_split.is_ok(), "{body:?}");
        let template = catch_unwind(move || {
            html! { script type="text/template" { (PreEscaped(b)) } p { "end" } }.into_string()
        });
        assert_eq!(script.is_ok(), template.is_ok(), "{body:?}");
        let b = body.to_string();
        let style = catch_unwind(move || {
            html! { style { (TrustedStyle::assume_trusted(b)) } p { "end" } }.into_string()
        });

        let expected = normalized(body);
        for (name, page) in [("script", script), ("script", template), ("style", style)] {
            let Ok(page) = page else { continue };
            *accepted += 1;
            for scripting in [true, false] {
                let els: Vec<_> = elements(&page, scripting)
                    .into_iter()
                    .filter(|e| !matches!(e.name.as_str(), "html" | "head" | "body"))
                    .collect();
                let shape: Vec<_> = els
                    .iter()
                    .map(|e| (e.name.as_str(), e.text.as_str()))
                    .collect();
                assert_eq!(
                    shape,
                    [(name, expected.as_str()), ("p", "end")],
                    "body {body:?} page {page:?}"
                );
            }
        }
    }

    #[test]
    fn tokenizer_states_fuzz() {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = catch_unwind(|| {
            let mut accepted = 0;
            // Every sequence of up to three tokens.
            for a in TOKENS {
                check_body(a, &mut accepted);
                for b in TOKENS {
                    check_body(&format!("{a}{b}"), &mut accepted);
                    for c in TOKENS {
                        check_body(&format!("{a}{b}{c}"), &mut accepted);
                    }
                }
            }
            // Longer random sequences.
            let mut rng = Rng(0x9e3779b97f4a7c15);
            for _ in 0..150_000 {
                let len = 4 + rng.below(9);
                let body: String = (0..len).map(|_| TOKENS[rng.below(TOKENS.len())]).collect();
                check_body(&body, &mut accepted);
            }
            accepted
        });
        std::panic::set_hook(hook);
        let accepted = result.expect("a body passed the check but did not round-trip");
        assert!(accepted > 100_000, "{accepted}");
    }

    /// The three data rows, under every parent content model, for every nasty
    /// input, with scripting on and off: no injected element or handler, and
    /// attacker text never sits in a script that runs or in a style.
    #[test]
    fn parents_do_not_turn_data_into_code() {
        let wrappers = all_wrappers();
        let mut pages = 0;
        for user in USER_INPUTS {
            let mut rows = vec![
                ("template", html! { script type="text/template" { (user) } }),
                (
                    "x-template",
                    html! { script type=" TEXT/X-Template\n" { "<b>" (user) "</b>" } },
                ),
            ];
            #[cfg(feature = "json")]
            {
                use maud::json::Json;
                let value = serde_json::json!({ *user: [*user] });
                rows.push((
                    "json",
                    html! { script type="application/json" { (Json(user)) } },
                ));
                rows.push((
                    "ld+json",
                    html! { script type="application/ld+json" { (value) } },
                ));
                rows.push((
                    "json after comment open",
                    html! { script type="application/json" { "<!--" (Json(user)) } },
                ));
            }
            for (row, block) in &rows {
                for (outer_name, outer) in &wrappers {
                    let one = outer(block.clone());
                    let page = html! { (one) p { "end" } }.into_string();
                    assert_eq!(
                        violations(&page),
                        Vec::<String>::new(),
                        "{row} in {outer_name}: {page}"
                    );
                    pages += 1;
                    for (inner_name, inner) in &wrappers {
                        let page =
                            html! { (outer(inner(block.clone()))) p { "end" } }.into_string();
                        assert_eq!(
                            violations(&page),
                            Vec::<String>::new(),
                            "{row} in {outer_name} > {inner_name}: {page}"
                        );
                        pages += 1;
                    }
                }
            }
        }
        assert!(pages > 10_000);
    }

    /// The same rows as a fragment (`el.innerHTML = response`, as htmx and
    /// friends do), for context elements with unusual insertion modes.
    #[test]
    fn fragment_contexts_do_not_turn_data_into_code() {
        use html5ever::{LocalName, QualName, parse_fragment};
        let contexts = [
            (ns!(html), "div"),
            (ns!(html), "body"),
            (ns!(html), "head"),
            (ns!(html), "html"),
            (ns!(html), "table"),
            (ns!(html), "tbody"),
            (ns!(html), "tr"),
            (ns!(html), "td"),
            (ns!(html), "colgroup"),
            (ns!(html), "select"),
            (ns!(html), "template"),
            (ns!(html), "frameset"),
            (ns!(html), "title"),
            (ns!(html), "textarea"),
            (ns!(html), "noscript"),
            (ns!(html), "iframe"),
            (ns!(html), "xmp"),
            (ns!(html), "plaintext"),
            (ns!(html), "p"),
            (ns!(html), "form"),
            (ns!(svg), "svg"),
            (ns!(svg), "foreignObject"),
            (ns!(svg), "title"),
            (ns!(svg), "desc"),
            (ns!(mathml), "math"),
            (ns!(mathml), "mi"),
            (ns!(mathml), "annotation-xml"),
        ];
        for user in USER_INPUTS {
            let mut rows = vec![
                html! { script type="text/template" { (user) } p { "end" } },
                html! { p { "a" } script type="text/x-template" { "<b>" (user) } },
            ];
            #[cfg(feature = "json")]
            rows.push(html! { script type="application/json" { (maud::json::Json(user)) } });
            for row in rows {
                let page = row.into_string();
                for (ns, name) in &contexts {
                    for scripting in [true, false] {
                        let dom = parse_fragment(
                            RcDom::default(),
                            ParseOpts::default(),
                            QualName::new(None, ns.clone(), LocalName::from(*name)),
                            Vec::new(),
                            scripting,
                        )
                        .one(page.as_str());
                        let mut els = Vec::new();
                        collect_public(&dom.document, &mut els);
                        let mut found = Vec::new();
                        violations_in(&els, &mut found);
                        assert_eq!(found, Vec::<String>::new(), "in {name}: {page}");
                    }
                }
            }
        }
    }

    /// Same-call nesting (not through a spliced `Markup`) under the parents
    /// that change the tokenizer state.
    #[test]
    fn same_call_parents() {
        let user = "</script></textarea></title><img src=x onerror=alert(document.domain)>";
        for page in [
            // `textarea`, `title`, `xmp`, `iframe`, `frameset` and `plaintext`
            // parents are compile errors now: see
            // `tests/warnings/raw-text-parents.rs`.
            html! { noscript { script type="text/template" { (user) } } p { "end" } },
            html! { template { script type="text/template" { (user) } } p { "end" } },
            html! { select { script type="text/template" { (user) } } p { "end" } },
            html! { table { script type="text/template" { (user) } } p { "end" } },
        ] {
            let page = page.into_string();
            assert_eq!(violations(&page), Vec::<String>::new(), "{page}");
        }
    }

    /// In an HTML-namespace template script, escaped data survives one
    /// `innerHTML` instantiation as text.
    #[test]
    fn template_instantiation_in_html_namespace() {
        for user in USER_INPUTS {
            for (name, wrap) in all_wrappers() {
                if name.contains("svg") || name.contains("math") {
                    continue;
                }
                let block = html! {
                    script type="text/template" { (PreEscaped("<div class=card>")) (user) (PreEscaped("</div>")) }
                };
                let page = html! { (wrap(block)) }.into_string();
                assert_eq!(
                    instantiation_violations(&page),
                    Vec::<String>::new(),
                    "{name}: {page}"
                );
            }
        }
    }

    /// The `Display` fallback exists only in the template row, where it is
    /// escaped.
    #[test]
    fn display_fallback_is_escaped_in_templates() {
        struct D;
        impl std::fmt::Display for D {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("</script><img src=x onerror=alert(1)>")
            }
        }
        let page =
            html! { script type="text/template" { (D) (format_args!("{}", D)) } p { "end" } }
                .into_string();
        assert!(!page[..page.len() - 20].contains("</script"), "{page}");
        assert_eq!(violations(&page), Vec::<String>::new());
    }

    #[cfg(feature = "json")]
    mod json {
        use super::*;
        use maud::json::Json;
        use serde::ser::{
            Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
            SerializeTupleVariant, Serializer,
        };
        use std::collections::{BTreeMap, HashMap};

        const EVIL: &str = "</script><!--<script>&<img src=x onerror=alert(document.domain)>";

        struct ViaDisplay;
        impl std::fmt::Display for ViaDisplay {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                // Split the forbidden text across writes.
                f.write_str("</scr")?;
                f.write_str("ipt><")?;
                std::fmt::Write::write_char(f, '<')?;
                f.write_str("!--<script>&")
            }
        }

        #[derive(serde::Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
        enum Variants {
            #[serde(rename = "</script><x>&")]
            Unit,
            #[serde(rename = "</script><n>&")]
            Newtype(&'static str),
            #[serde(rename = "</script><t>&")]
            Tuple(char, char),
            #[serde(rename = "</script><s>&")]
            Struct {
                #[serde(rename = "</script><f>&")]
                field: char,
            },
        }

        #[derive(serde::Serialize)]
        #[serde(rename = "</script><name>")]
        struct Flat {
            #[serde(rename = "</script><k>&")]
            a: char,
            #[serde(flatten)]
            rest: BTreeMap<String, serde_json::Value>,
            #[serde(flatten)]
            inner: Inner,
        }

        #[derive(serde::Serialize)]
        #[serde(tag = "</script><tag>&")]
        enum Tagged {
            #[serde(rename = "</script><v>&")]
            V {
                #[serde(with = "as_bytes")]
                b: &'static str,
            },
        }

        mod as_bytes {
            pub fn serialize<S: serde::Serializer>(v: &str, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bytes(v.as_bytes())
            }
        }

        #[derive(serde::Serialize)]
        struct Inner {
            #[serde(rename = "<&>")]
            c: Variants,
        }

        /// Calls every `Serializer` entry point that takes text or bytes.
        struct Everything;
        impl Serialize for Everything {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut seq = s.serialize_seq(None)?;
                seq.serialize_element(EVIL)?;
                seq.serialize_element(&'<')?;
                seq.serialize_element(&'>')?;
                seq.serialize_element(&'&')?;
                seq.serialize_element(&Bytes(EVIL.as_bytes()))?;
                seq.serialize_element(&Bytes(b"\xff\xfe</script>"))?;
                seq.serialize_element(&Collect)?;
                seq.serialize_element(&Variants::Unit)?;
                seq.serialize_element(&Variants::Newtype(EVIL))?;
                seq.serialize_element(&Variants::Tuple('<', '&'))?;
                seq.serialize_element(&Variants::Struct { field: '<' })?;
                seq.serialize_element(&Manual)?;
                seq.serialize_element(&FakeRaw(EVIL))?;
                seq.serialize_element(&FakeRaw("\"</script>\""))?;
                seq.serialize_element(&FakeNumber(EVIL))?;
                seq.serialize_element(&Keys)?;
                seq.serialize_element(&f64::NAN)?;
                seq.serialize_element(&f32::INFINITY)?;
                seq.serialize_element(&i128::MIN)?;
                seq.serialize_element(&u128::MAX)?;
                seq.serialize_element(std::path::Path::new(EVIL))?;
                seq.serialize_element(&std::net::Ipv4Addr::LOCALHOST)?;
                seq.serialize_element(&Flat {
                    a: '<',
                    rest: BTreeMap::from([(EVIL.to_string(), serde_json::json!(EVIL))]),
                    inner: Inner { c: Variants::Unit },
                })?;
                seq.serialize_element(&Tagged::V { b: "<&>" })?;
                seq.end()
            }
        }

        struct Bytes<'a>(&'a [u8]);
        impl Serialize for Bytes<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bytes(self.0)
            }
        }

        struct Collect;
        impl Serialize for Collect {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(&ViaDisplay)
            }
        }

        struct Manual;
        impl Serialize for Manual {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut st = s.serialize_struct_variant(EVIL_NAME, 0, EVIL_NAME, 1)?;
                st.serialize_field(EVIL_NAME, &ManualTuple)?;
                st.end()
            }
        }
        const EVIL_NAME: &str = "</script><m>&";

        struct ManualTuple;
        impl Serialize for ManualTuple {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut t = s.serialize_tuple_variant(EVIL_NAME, 0, EVIL_NAME, 1)?;
                t.serialize_field(&UnitVariant)?;
                t.end()
            }
        }

        struct UnitVariant;
        impl Serialize for UnitVariant {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_newtype_struct(EVIL_NAME, &UnitStruct)
            }
        }
        struct UnitStruct;
        impl Serialize for UnitStruct {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_unit_variant(EVIL_NAME, 0, EVIL_NAME)
            }
        }

        /// Imitates `RawValue`'s private protocol with text that is not JSON.
        struct FakeRaw(&'static str);
        impl Serialize for FakeRaw {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut st = s.serialize_struct("$serde_json::private::RawValue", 1)?;
                st.serialize_field("$serde_json::private::RawValue", &RawField(self.0))?;
                st.end()
            }
        }
        /// The raw emitter also accepts `collect_str`.
        struct RawField(&'static str);
        impl Serialize for RawField {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                if self.0.starts_with('"') {
                    s.serialize_str(self.0)
                } else {
                    s.collect_str(self.0)
                }
            }
        }

        /// Imitates `Number`'s private protocol (live with `arbitrary_precision`).
        struct FakeNumber(&'static str);
        impl Serialize for FakeNumber {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut st = s.serialize_struct("$serde_json::private::Number", 1)?;
                st.serialize_field("$serde_json::private::Number", self.0)?;
                st.end()
            }
        }

        /// Map keys of every type the key serializer accepts.
        struct Keys;
        impl Serialize for Keys {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut map = s.serialize_map(None)?;
                map.serialize_entry(EVIL, EVIL)?;
                map.serialize_entry(&'<', &'>')?;
                map.serialize_entry(&Collect, &Collect)?;
                map.serialize_entry(&Variants::Unit, &0)?;
                map.serialize_entry(&UnitVariant, &0)?;
                map.serialize_entry(&-1i8, &0)?;
                map.serialize_entry(&u128::MAX, &0)?;
                map.serialize_entry(&true, &0)?;
                map.serialize_entry(&1.5f64, &0)?;
                map.serialize_entry(&String::from(EVIL), &0)?;
                map.serialize_key(&std::borrow::Cow::Borrowed(EVIL))?;
                map.serialize_value(&FakeRaw(EVIL))?;
                map.end()
            }
        }

        fn check(page: Markup) {
            let page = page.into_string();
            let body = page
                .split_once('>')
                .unwrap()
                .1
                .rsplit_once("</script><p>end</p>")
                .expect(&page)
                .0;
            assert!(!body.contains(['<', '>', '&']), "{body}");
            assert!(body.is_ascii() || std::str::from_utf8(body.as_bytes()).is_ok());
            assert_eq!(violations(&page), Vec::<String>::new(), "{page}");
            let summary = summarize(PreEscaped(page.clone()));
            assert_eq!(summary.len(), 2, "{page}");
            assert_eq!(summary[1], "p{end}");
        }

        #[test]
        fn every_serializer_path_is_escaped() {
            check(html! { script type="application/json" { (Json(Everything)) } p { "end" } });
            check(html! { script type="application/ld+json" { (Json(&Everything)) } p { "end" } });
            check(
                html! { script type="application/json" { (Box::new(Json(Everything))) } p { "end" } },
            );
            check(html! { script type="application/json" { (Json(FakeRaw(EVIL))) } p { "end" } });
            check(
                html! { script type="application/json" { (Json(FakeNumber(EVIL))) } p { "end" } },
            );
            check(html! { script type="application/json" { (Json(Collect)) } p { "end" } });
            check(
                html! { script type="application/json" { (Json(Bytes(EVIL.as_bytes()))) } p { "end" } },
            );
            let map = HashMap::from([(Variants::Unit, Variants::Struct { field: '<' })]);
            check(html! { script type="application/json" { (Json(map)) } p { "end" } });
            let dyn_json: &dyn maud::RenderIn<maud::ctx::JsonScript> = &Json(Everything);
            check(html! { script type="application/json" { (dyn_json) } p { "end" } });
        }

        /// JSON output holds no raw control characters, so data cannot start
        /// an ISO-2022-JP escape sequence on a page without a charset.
        #[test]
        fn json_has_no_raw_control_characters() {
            let all: String = (0u8..0x20).map(char::from).chain(['\x7f']).collect();
            let page = html! { script type="application/json" { (Json(&all)) } }.into_string();
            assert!(!page.bytes().any(|b| b < 0x20), "{page:?}");
        }

        /// `RawValue` built without `serde_json` validating it: the escaping
        /// does not depend on the text being well-formed JSON.
        #[test]
        fn unvalidated_raw_value() {
            use serde::{
                Deserialize,
                de::value::{Error, MapDeserializer},
            };
            use serde_json::value::RawValue;
            let de = MapDeserializer::<_, Error>::new(std::iter::once((
                "$serde_json::private::RawValue",
                EVIL,
            )));
            match Box::<RawValue>::deserialize(de) {
                Ok(raw) => {
                    assert_eq!(raw.get(), EVIL);
                    check(html! { script type="application/json" { (raw) } p { "end" } });
                    check(html! { script type="application/json" { (Json(&raw)) } p { "end" } });
                }
                Err(e) => println!("could not build an unvalidated RawValue: {e}"),
            }
        }

        /// A literal can supply the `<` that JSON output never contains; the
        /// run-time check catches every completion.
        #[test]
        fn literal_prefix_plus_fake_raw_panics() {
            assert_panics(|| {
                let _ = html! { script type="application/json" { "<" (Json(FakeRaw("/script"))) } };
            });
            assert_panics(|| {
                let _ =
                    html! { script type="application/json" { "<!--<" (Json(FakeRaw("script"))) } };
            });
            assert_panics(|| {
                let _ = html! { script type="application/json" { "</" (Json(Collect2)) } };
            });
            struct Collect2;
            impl Serialize for Collect2 {
                fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    let mut st = s.serialize_struct("$serde_json::private::RawValue", 1)?;
                    st.serialize_field("$serde_json::private::RawValue", "SCRIPT")?;
                    st.end()
                }
            }
        }
    }
}

/// Findings from attack 3 that are fixed. A3-1 (string literal attribute
/// names), A3-3 (`from_const_internal`) and A3-4 (`script` inside RCDATA
/// parents) are now compile errors: see `tests/warnings/raw-text-attr-names.rs`,
/// `tests/warnings/raw-text-parents.rs` and `tests/warnings/trusted-hidden.rs`.
mod a3_fixed {
    use super::{a3::*, *};

    /// A3-2, a known limitation (see the `ctx` module docs). A template script
    /// spliced into `svg` or `math` from another `html!` call has its
    /// character references decoded, so instantiating the template builds the
    /// spliced markup. The macro cannot see across calls. This asserts the
    /// current output, so a change in behaviour is noticed.
    #[test]
    fn template_instantiation_in_foreign_content() {
        let user = "<img src=x onerror=alert(document.domain)>";
        let template = html! {
            script type="text/template" id="card" { "Hello, " (user) "!" }
        };
        // In the HTML namespace the instantiated text stays text.
        let control = html! { div { (template.clone()) } }.into_string();
        assert_eq!(
            instantiation_violations(&control),
            Vec::<String>::new(),
            "{control}"
        );
        for page in [
            html! { svg { (template.clone()) } },
            html! { math { (template.clone()) } },
            html! { svg { title { "t" } g { (template.clone()) } } },
        ] {
            let page = page.into_string();
            // The page itself is safe...
            assert_eq!(violations(&page), Vec::<String>::new(), "{page}");
            // ...but the instantiated template is not: it builds the `<img>`
            // with its `onerror` handler.
            assert_eq!(instantiation_violations(&page).len(), 2, "{page}");
        }
    }

    /// A3-3 (part). The constructor the macros use is reachable, under a
    /// name that a search for `assume_trusted` finds. Like the named escape
    /// hatch, it is still covered by the check when the element closes.
    #[test]
    fn macro_constructor_is_still_checked_when_the_element_closes() {
        let user = String::from("</script><img src=x onerror=alert(document.domain)>");
        let trusted = maud::macro_private::assume_trusted_script_const(String::leak(user));
        assert_panics(move || {
            let _ = html! { script { (trusted) } };
        });
    }
}

/// Pre-existing behaviour that the context scheme does not cover yet.
mod a3_preexisting {
    use super::{a3::*, *};

    /// A3-5 (from #506, documented there as a limitation). `Json` is `Render`,
    /// so it is accepted in an attribute value, where its quotes are written
    /// as they are. Attacker text in the JSON then adds attributes.
    #[cfg(feature = "json")]
    #[test]
    #[ignore = "pre-existing; fixed separately by lambda-fairy/maud#510"]
    fn json_in_attribute_injects_attributes() {
        let user = "x autofocus onfocus=alert(document.domain) y";
        let page = html! { input data-config=(maud::json::Json(user)); }.into_string();
        assert_eq!(violations(&page), Vec::<String>::new(), "{page}");
    }

    /// Design caveat, not a regression: HTML escaping does nothing about
    /// template expressions, and nothing about `'` in a single-quoted
    /// attribute of a `PreEscaped` template. Both are asserted as they are.
    #[test]
    fn template_row_caveats() {
        let user = "{{constructor.constructor('alert(document.domain)')()}}";
        let page = html! { script type="text/x-template" id="t" { (user) } }.into_string();
        assert_eq!(script_text(PreEscaped(page)), user);

        let user = "' onmouseover='alert(document.domain)";
        let page = html! {
            script type="text/template" { (PreEscaped("<b title='")) (user) (PreEscaped("'>x</b>")) }
        }
        .into_string();
        assert_eq!(instantiation_violations(&page).len(), 1, "{page}");
    }
}

/// Negative trait checks that need `serde_json`, which the `trybuild` cases
/// cannot name. Each line fails to compile if the type gains the impl.
#[test]
fn a3_not_render_in() {
    use maud::{
        Render, TrustedStyle,
        ctx::{JsonScript, Script, Style},
    };
    use std::{
        borrow::Cow,
        fmt::{Arguments, Display},
        rc::Rc,
        sync::Arc,
    };

    macro_rules! assert_not_render_in {
        ($ctx:ty: $($ty:ty),* $(,)?) => {$({
            trait AmbiguousIfImpl<A> { fn item() {} }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            impl<T: ?Sized + maud::RenderIn<$ctx>> AmbiguousIfImpl<u8> for T {}
            let _ = <$ty as AmbiguousIfImpl<_>>::item;
        })*};
    }

    assert_not_render_in!(Script:
        str, String, &str, &String, Cow<'static, str>, char, u8, i32, f64, bool, (),
        Markup, PreEscaped<&'static str>, Arguments<'static>, Box<dyn Render>, &dyn Render,
        Arc<dyn Render>, Box<dyn Display>, Box<str>, Arc<str>, Rc<TrustedScript>,
        Option<TrustedScript>, TrustedStyle, &TrustedStyle, Box<TrustedStyle>,
        dyn maud::RenderIn<Style>, dyn maud::RenderIn<JsonScript>,
    );
    assert_not_render_in!(Style:
        str, String, &str, Cow<'static, str>, i32, Markup, PreEscaped<String>,
        Box<dyn Render>, TrustedScript, &TrustedScript, Arc<TrustedScript>,
        dyn maud::RenderIn<Script>,
    );
    assert_not_render_in!(JsonScript:
        str, String, &str, Cow<'static, str>, i32, bool, Markup, PreEscaped<String>,
        Box<dyn Render>, TrustedScript, TrustedStyle, dyn maud::RenderIn<Script>,
    );
    #[cfg(feature = "json")]
    {
        use maud::json::Json;
        use serde_json::{Map, Number, Value, value::RawValue};
        assert_not_render_in!(Script:
            Json<String>, Json<&str>, Json<Value>, &Json<i32>, Value, &Value, Box<Value>,
            RawValue, Box<RawValue>, &RawValue, Number, Map<String, Value>,
        );
        assert_not_render_in!(Style: Json<String>, Value, RawValue, Box<RawValue>);
        assert_not_render_in!(JsonScript:
            Number, Map<String, Value>, Option<Value>, Vec<Value>, Rc<Value>,
        );
    }
}

/// Type-system attacks. Every case must fail to compile; `trybuild` reports a
/// case that compiles as a failure.
#[test]
fn a3_type_system_cases_do_not_compile() {
    const CASES: &[(&str, &str)] = &[
        (
            "impl-renderin-own-type",
            r#"
            use maud::{RenderIn, ctx::Script};
            struct Mine(String);
            impl RenderIn<Script> for Mine {
                fn render_in(&self, b: &mut String) { b.push_str(&self.0) }
            }
            fn main() {}
        "#,
        ),
        (
            "impl-renderin-box-of-local",
            r#"
            use maud::{RenderIn, ctx::Script};
            struct Mine(String);
            impl RenderIn<Script> for Box<Mine> {
                fn render_in(&self, b: &mut String) { b.push_str(&self.0) }
            }
            fn main() {}
        "#,
        ),
        (
            "impl-renderin-ref-of-local",
            r#"
            use maud::{RenderIn, ctx::Style};
            struct Mine(String);
            impl RenderIn<Style> for &Mine {
                fn render_in(&self, b: &mut String) { b.push_str(&self.0) }
            }
            fn main() {}
        "#,
        ),
        (
            "impl-renderin-own-ctx",
            r#"
            use maud::RenderIn;
            enum MyCtx {}
            struct Mine(String);
            impl RenderIn<MyCtx> for Mine {
                fn render_in(&self, b: &mut String) { b.push_str(&self.0) }
            }
            fn main() {}
        "#,
        ),
        (
            "name-sealed",
            r#"
            struct Mine;
            impl maud::ctx::sealed::Sealed<maud::ctx::Script> for Mine {}
            fn main() {}
        "#,
        ),
        (
            "generic-render",
            r#"
            use maud::{html, Markup, Render};
            fn f<T: Render>(t: T) -> Markup { html! { script { (t) } } }
            fn main() { f("alert(1)"); }
        "#,
        ),
        (
            "generic-display",
            r#"
            use maud::{html, Markup};
            fn f<T: std::fmt::Display>(t: T) -> Markup { html! { script { (t) } } }
            fn main() { f("alert(1)"); }
        "#,
        ),
        (
            "box-dyn-render",
            r#"
            use maud::{html, Render};
            fn main() {
                let b: Box<dyn Render> = Box::new("alert(1)");
                html! { script { (b) } };
            }
        "#,
        ),
        (
            "ref-dyn-display",
            r#"
            use maud::html;
            fn main() {
                let d: &dyn std::fmt::Display = &"alert(1)";
                html! { script { (d) } };
            }
        "#,
        ),
        (
            "format-args",
            r#"
            use maud::html;
            fn main() {
                let u = String::from("alert(1)");
                html! { script { (format_args!("{u}")) } };
            }
        "#,
        ),
        (
            "maud-display",
            r#"
            use maud::html;
            fn main() { html! { style { (maud::display("a{}")) } }; }
        "#,
        ),
        (
            "deref-to-str",
            r#"
            use maud::html;
            struct W(String);
            impl std::ops::Deref for W { type Target = str; fn deref(&self) -> &str { &self.0 } }
            fn main() { html! { script { (W(String::new())) } }; }
        "#,
        ),
        (
            "unsize-to-dyn-renderin",
            r#"
            use maud::{html, RenderIn, ctx::Script};
            fn main() {
                let s = String::from("alert(1)");
                let d: &dyn RenderIn<Script> = &s;
                html! { script { (d) } };
            }
        "#,
        ),
        (
            "private-field",
            r#"
            fn main() { let _ = maud::TrustedScript(std::borrow::Cow::Borrowed("x")); }
        "#,
        ),
        (
            "from-string",
            r#"
            fn main() { let _: maud::TrustedScript = String::from("x").into(); }
        "#,
        ),
        (
            "default",
            r#"
            fn main() { let _ = <maud::TrustedScript as Default>::default(); }
        "#,
        ),
        (
            "trusted-macro-leak",
            r#"
            fn main() {
                let u = String::from("alert(1)");
                let _ = maud::trusted_script!(String::leak(u));
            }
        "#,
        ),
        (
            "trusted-macro-static-mut",
            r#"
            static mut CODE: &str = "x";
            fn main() { let _ = maud::trusted_script!(unsafe { CODE }); }
        "#,
        ),
        (
            "trusted-style-in-script",
            r#"
            use maud::html;
            fn main() { html! { script { (maud::trusted_style!("a{}")) } }; }
        "#,
        ),
        (
            "trusted-script-in-json",
            r#"
            use maud::html;
            fn main() { html! { script type="application/json" { (maud::trusted_script!("1")) } }; }
        "#,
        ),
        (
            "string-in-json",
            r#"
            use maud::html;
            fn main() { html! { script type="application/ld+json" { (String::from("1")) } }; }
        "#,
        ),
        (
            "preescaped-in-json",
            r#"
            use maud::{html, PreEscaped};
            fn main() { html! { script type="application/json" { (PreEscaped("1")) } }; }
        "#,
        ),
        (
            "int-in-json",
            r#"
            use maud::html;
            fn main() { html! { script type="application/json" { (1) } }; }
        "#,
        ),
        (
            "json-in-script",
            r#"
            use maud::{html, json::Json};
            fn main() { html! { script { "var a = " (Json("x")) ";" } }; }
        "#,
        ),
        (
            "json-in-module-script",
            r#"
            use maud::{html, json::Json};
            fn main() { html! { script type="module" { (Json("x")) } }; }
        "#,
        ),
        (
            "json-in-style",
            r#"
            use maud::{html, json::Json};
            fn main() { html! { style { (Json("x")) } }; }
        "#,
        ),
        (
            "json-with-params-is-executable",
            r#"
            use maud::{html, json::Json};
            fn main() { html! { script type="application/json;charset=utf-8" { (Json(1)) } }; }
        "#,
        ),
        (
            "template-with-dynamic-type",
            r#"
            use maud::html;
            fn main() { let t = "text/template"; html! { script type=(t) { ("user") } }; }
        "#,
        ),
        (
            "template-with-block-type",
            r#"
            use maud::html;
            fn main() { html! { script type={ "text/" "template" } { ("user") } }; }
        "#,
        ),
        (
            "template-with-toggled-second-type",
            r#"
            use maud::html;
            fn main() { html! { script type=[None::<&str>] type="text/template" { ("user") } }; }
        "#,
        ),
        (
            "template-with-uppercase-second-type",
            r#"
            use maud::html;
            fn main() { html! { script TYPE="module" type="text/template" { ("user") } }; }
        "#,
        ),
        (
            "template-with-raw-ident-type",
            r#"
            use maud::html;
            fn main() { html! { script r#type="text/template" { ("user") } }; }
        "#,
        ),
        (
            "template-with-nbsp",
            r#"
            use maud::html;
            fn main() { html! { script type="text/template\u{a0}" { ("user") } }; }
        "#,
        ),
        (
            "template-with-nul",
            r#"
            use maud::html;
            fn main() { html! { script type="text/template\0" { ("user") } }; }
        "#,
        ),
        (
            "template-with-vertical-tab",
            r#"
            use maud::html;
            fn main() { html! { script type="\u{b}text/template" { ("user") } }; }
        "#,
        ),
        (
            "uppercase-script-element",
            r#"
            use maud::html;
            fn main() { html! { SCRIPT { ("user") } }; }
        "#,
        ),
        (
            "uppercase-style-element",
            r#"
            use maud::html;
            fn main() { html! { Style { ("user") } }; }
        "#,
        ),
        (
            "void-template-script",
            r#"
            use maud::html;
            fn main() { html! { script type="text/template"; }; }
        "#,
        ),
        (
            "check-in-generic-fn",
            r#"
            #[allow(dead_code)]
            fn never_called<T>() -> maud::TrustedScript { maud::trusted_script!("</script>") }
            fn main() {}
        "#,
        ),
        (
            "check-in-closure-in-impl",
            r#"
            #[allow(dead_code)]
            struct G<T>(T);
            impl<T> G<T> {
                #[allow(dead_code)]
                fn f() -> impl Fn() -> maud::TrustedStyle { || maud::trusted_style!(concat!("</sty", "le>")) }
            }
            fn main() {}
        "#,
        ),
        (
            "literal-split-by-let",
            r#"
            use maud::html;
            fn main() { html! { script { "</scr" @let _x = 1; { "ipt>" } } }; }
        "#,
        ),
    ];

    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("a3-cases");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, source) in CASES {
        #[cfg(not(feature = "json"))]
        if source.contains("json") {
            continue;
        }
        std::fs::write(dir.join(format!("{name}.rs")), source).unwrap();
    }
    // Without expected-output files `trybuild` fails every case as "wip".
    // In overwrite mode it records the errors next to the cases instead, and
    // still fails a case that compiles.
    // SAFETY: no other test in this binary reads or writes this variable.
    unsafe { std::env::set_var("TRYBUILD", "overwrite") };
    let cases = trybuild::TestCases::new();
    cases.compile_fail(dir.join("*.rs"));
}

// ===================== attack2 =====================
mod attack2 {
    use super::*;
    use maud::Render;

    /// A1: element name smuggled through a string-literal name fragment.
    #[test]
    fn a1_element_name_literal_with_whitespace_is_a_script() {
        let user = "alert(document.domain)";
        let out = html! { "script " { (user) } };
        // HELD: a leading string literal is text, not an element name.
        assert_eq!(summarize(out), Vec::<String>::new());
    }

    #[test]
    fn a1b_element_name_literal_slash() {
        let user = "alert(1)";
        let out = html! { "script/" { (user) } };
        assert_eq!(summarize(out), Vec::<String>::new()); // HELD
    }

    // A2, A2b, A2c (string literal attribute names) are compile errors now:
    // see `tests/warnings/raw-text-attr-names.rs`.

    /// A3: `continue` inside a script body used to skip both the run-time
    /// check and the end tag. The guard now checks the body, drops it because
    /// it fails, and closes the element.
    #[test]
    fn a3_continue_skips_check() {
        let items: Vec<(TrustedScript, Option<TrustedScript>)> = vec![(
            TrustedScript::assume_trusted("</script><img src=x onerror=alert(1)>"),
            None,
        )];
        let out = html! {
            @for (a, b) in &items {
                script { (a) (match b { Some(b) => b, None => continue }) }
            }
            p { "end" }
        };
        assert_eq!(out.into_string(), "<script></script><p>end</p>");
    }

    /// A3b: template row with PreEscaped and `break` from @let: closed and
    /// checked too.
    #[test]
    #[allow(clippy::diverging_sub_expression)]
    fn a3b_break_in_let_template_row() {
        let user = "</script><img src=x onerror=alert(1)>";
        let out = html! {
            @for _ in 0..1 {
                script type="text/template" { (PreEscaped(user)) @let _x: () = { break; }; }
            }
        };
        assert_eq!(
            out.into_string(),
            r#"<script type="text/template"></script>"#
        );
    }

    /// A4: custom Render in the template row rewrites the buffer before the
    /// body start, padded so the slice stays in range. A `Render` impl is
    /// trusted like `PreEscaped`, so this is asserted as it is.
    struct Rewind(&'static str);
    impl Render for Rewind {
        fn render_to(&self, buf: &mut String) {
            let start = buf.rfind("<script").unwrap();
            let body_start = buf.len();
            buf.truncate(start);
            buf.push_str(self.0);
            while buf.len() < body_start {
                buf.push(' ');
            }
        }
    }
    #[test]
    fn a4_render_rewrites_before_body_start() {
        let out = html! {
            script type="text/template" { (Rewind("<img src=x onerror=alert(1)><script>")) }
        };
        assert!(summarize(out).iter().any(|e| e.starts_with("img")));
    }

    /// A5: custom Render that truncates: does the slice panic (DoS) or worse?
    struct Clear;
    impl Render for Clear {
        fn render_to(&self, buf: &mut String) {
            buf.clear();
        }
    }
    #[test]
    fn a5_truncate_panics_on_slice() {
        assert_panics(|| {
            let _ = html! { script type="text/template" { (Clear) } };
        });
    }

    /// A6: panic mid-body caught by catch_unwind: buffer is html!-local.
    #[test]
    fn a6_panic_mid_body_buffer_dropped() {
        let r = catch_unwind(|| {
            html! {
                script type="text/template" { (PreEscaped("<!--")) (PreEscaped({ panic!("x"); #[allow(unreachable_code)] "" })) }
            }
        });
        assert!(r.is_err());
    }

    /// A7: user macro_rules passing literals as fragments.
    macro_rules! wrap_lit {
        ($b:literal) => {
            html! { script { $b } }
        };
    }
    macro_rules! wrap_expr {
        ($b:expr) => {
            html! { script { $b } }
        };
    }
    macro_rules! wrap_ty {
        ($t:literal, $e:expr) => {
            html! { script type=$t { ($e) } }
        };
    }
    macro_rules! wrap_name {
        ($n:ident, $e:expr) => {
            html! { $n { ($e) } }
        };
    }
    #[test]
    fn a7_macro_fragments() {
        // These must be compile errors if the check sees through None groups; see notes.
        let _ = wrap_lit!("ok");
        let _ = wrap_expr!("ok");
        let _ = wrap_ty!("text/template", "x<y");
        let _ = wrap_name!(script, trusted_script!("ok"));
    }

    /// A8: trusted_script! with concat!/include_str!/env!.
    #[test]
    fn a8_trusted_script_const_forms() {
        let _ = trusted_script!(concat!("a", "b"));
        let _ = trusted_script!(env!("CARGO_PKG_NAME"));
        const P: &str = "x";
        let _ = trusted_script!(P);
    }

    /// A10: style element name via literal.
    #[test]
    fn a10_style_literal_name() {
        let user = "</style><img src=x onerror=alert(1)>";
        // Normal context escapes `<`, so no breakout; check it.
        let out = html! { "style " { (user) } };
        let s = summarize(out);
        assert!(!s.iter().any(|e| e.starts_with("img")), "{s:?}");
    }

    /// A11: nested html! inside a splice in a script body does not disturb the
    /// outer body_start / output.
    #[test]
    fn a11_nested_html_in_splice() {
        assert_panics(|| {
            let _ = html! {
                script {
                    (TrustedScript::assume_trusted("</scr"))
                    ({ let _inner = html! { script { (trusted_script!("x")) } p { "y" } }; TrustedScript::assume_trusted("ipt>") })
                }
            };
        });
    }
}
