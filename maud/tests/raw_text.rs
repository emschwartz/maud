use maud::{PreEscaped, TrustedScript, TrustedStyle, html, trusted_script, trusted_style};
use std::panic::catch_unwind;

#[test]
fn script_literals_are_raw() {
    let result = html! { script { "if (a < b && c) { go(\"x\") }" } };
    assert_eq!(
        result.into_string(),
        r#"<script>if (a < b && c) { go("x") }</script>"#
    );
}

#[test]
fn script_types_that_run_keep_literals_raw() {
    let result = html! {
        script type="module" { "a<b" }
        script type="TEXT/JAVASCRIPT" { "a<b" }
        script type=" text/javascript " { "a<b" }
        script type="" { "a<b" }
        script type="importmap" { "{}" }
        script type=("text/template") { "a<b" }
    };
    assert_eq!(
        result.into_string(),
        concat!(
            r#"<script type="module">a<b</script>"#,
            r#"<script type="TEXT/JAVASCRIPT">a<b</script>"#,
            r#"<script type=" text/javascript ">a<b</script>"#,
            r#"<script type="">a<b</script>"#,
            r#"<script type="importmap">{}</script>"#,
            // A spliced `type` could be anything, so it counts as executable.
            r#"<script type="text/template">a<b</script>"#,
        )
    );
}

#[test]
fn script_trusted_splices() {
    const HELLO: &str = "console.log('hello');";
    let dynamic = TrustedScript::assume_trusted(String::from("go();"));
    let result = html! {
        script {
            (trusted_script!(HELLO))
            (trusted_script!(concat!("let x = 1", ";")))
            (dynamic)
            (&dynamic)
        }
    };
    assert_eq!(
        result.into_string(),
        "<script>console.log('hello');let x = 1;go();go();</script>"
    );
}

#[test]
fn style_literals_are_raw() {
    let color = trusted_style!("color: red;");
    let result = html! {
        style { "a > b { " (color) " }" }
    };
    assert_eq!(result.into_string(), "<style>a > b { color: red; }</style>");
}

#[test]
fn style_assume_trusted() {
    let css = TrustedStyle::assume_trusted(format!("p {{ margin: {}px }}", 4));
    assert_eq!(css.as_str(), "p { margin: 4px }");
    let result = html! { style { (css) } };
    assert_eq!(result.into_string(), "<style>p { margin: 4px }</style>");
}

#[test]
fn template_script_escapes_as_before() {
    let name = "<b>";
    let result = html! {
        script type="text/template" { "<div>" (name) "</div>" }
        script type=" Text/X-Template " { "<p>" }
    };
    assert_eq!(
        result.into_string(),
        concat!(
            r#"<script type="text/template">&lt;div&gt;&lt;b&gt;&lt;/div&gt;</script>"#,
            r#"<script type=" Text/X-Template ">&lt;p&gt;</script>"#,
        )
    );
}

#[test]
fn control_flow_in_script() {
    let items = [1, 2, 3];
    let flag = true;
    let n = 2;
    let result = html! {
        script {
            "let a = [];"
            @for i in &items {
                @let code = TrustedScript::assume_trusted(format!("a.push({i});"));
                (code)
            }
            @if flag {
                "if (a.length > 1) {}"
            } @else {
                (trusted_script!("never();"))
            }
            @match n {
                1 => "one();",
                _ => { "other(" (trusted_script!("2")) ");" }
            }
            @let mut count = 0;
            @while count < 2 {
                "x<" @let _ = { count += 1; count };
            }
            {
                @let inner = trusted_script!("inner();");
                (inner)
            }
        }
    };
    assert_eq!(
        result.into_string(),
        "<script>let a = [];a.push(1);a.push(2);a.push(3);if (a.length > 1) {}\
         other(2);x<x<inner();</script>"
    );
}

#[test]
fn control_flow_in_style() {
    let dark = false;
    let result = html! {
        style {
            @if dark { "body { color: white }" } @else { "body { color: black }" }
        }
    };
    assert_eq!(result.into_string(), "<style>body { color: black }</style>");
}

#[test]
fn comment_open_alone_is_allowed() {
    // JavaScript treats `<!--` as a line comment, and on its own it cannot
    // change where the element ends.
    let result = html! { script { "<!-- legacy\nrun();\n-->" } };
    assert_eq!(
        result.into_string(),
        "<script><!-- legacy\nrun();\n--></script>"
    );
}

fn panic_message(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
    let err = catch_unwind(f).expect_err("expected a panic");
    err.downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap()
}

#[test]
fn runtime_check_assume_trusted() {
    let message = panic_message(|| {
        let evil = TrustedScript::assume_trusted("</ScRiPt><img src=x onerror=alert(1)>");
        let _ = html! { script { (evil) } };
    });
    assert!(message.contains("`</ScRiPt`"), "{message}");

    let message = panic_message(|| {
        let evil = TrustedScript::assume_trusted("<!--<script>");
        let _ = html! { script { (evil) } };
    });
    assert!(message.contains("`<script`"), "{message}");

    let message = panic_message(|| {
        let evil = TrustedStyle::assume_trusted("</style><script>alert(1)</script>");
        let _ = html! { style { (evil) } };
    });
    assert!(message.contains("`</style`"), "{message}");
}

#[test]
fn runtime_check_joins_literals_and_splices() {
    // Neither half is a problem on its own.
    panic_message(|| {
        let _ = html! { script { "<" (trusted_script!("/script>")) } };
    });
    panic_message(|| {
        let _ = html! { script { "</scr" @if true { "ipt>" } } };
    });
    panic_message(|| {
        let _ = html! { script { @for part in ["</scr", "ipt>"] { (TrustedScript::assume_trusted(part)) } } };
    });
    panic_message(|| {
        let _ = html! { style { "</sty" { @let x = 1; @if x == 1 { "le>" } } } };
    });
}

#[test]
fn runtime_check_template_script() {
    let message = panic_message(|| {
        let _ = html! {
            script type="text/template" { (PreEscaped("</script><img src=x onerror=alert(1)>")) }
        };
    });
    assert!(message.contains("`</script`"), "{message}");
}

#[test]
fn runtime_check_only_covers_the_body() {
    let result = html! {
        p { (PreEscaped("</script>")) }
        script { (trusted_script!("ok();")) }
        p { (PreEscaped("</script>")) }
    };
    assert_eq!(
        result.into_string(),
        "<p></script></p><script>ok();</script><p></script></p>"
    );
}

#[test]
fn script_attributes_are_still_escaped() {
    let src = "a\"b.js";
    let result = html! { script src=(src) {} };
    assert_eq!(
        result.into_string(),
        r#"<script src="a&quot;b.js"></script>"#
    );
}

#[test]
fn element_names_ignore_case() {
    let result = html! {
        SCRIPT { "a<b" }
        Style { "a>b" }
    };
    assert_eq!(
        result.into_string(),
        "<SCRIPT>a<b</SCRIPT><Style>a>b</Style>"
    );
}

/// Leaving a body early with `continue`, `break`, `return` or `?` still writes
/// the end tag.
#[test]
#[allow(clippy::diverging_sub_expression)]
fn early_exit_closes_the_element() {
    let result = html! {
        @for i in 0..3 {
            script {
                "a(" (trusted_script!("1")) ");"
                @if i == 1 { (match None::<TrustedScript> { Some(t) => t, None => continue }) }
                @if i == 2 { @let _x: () = { break; }; }
                "b();"
            }
        }
        p { "end" }
    };
    assert_eq!(
        result.into_string(),
        "<script>a(1);b();</script><script>a(1);</script><script>a(1);</script><p>end</p>"
    );

    fn with_question_mark(code: Option<TrustedScript>) -> Option<String> {
        let mut out = String::new();
        let markup = html! { script { "x();" (code?) } };
        out.push_str(&markup.into_string());
        Some(out)
    }
    assert_eq!(with_question_mark(None), None);
    assert_eq!(
        with_question_mark(Some(trusted_script!("y();"))).as_deref(),
        Some("<script>x();y();</script>")
    );
}

/// An early exit from a body that fails the check drops the body instead of
/// writing it, since the guard cannot panic while another panic unwinds.
#[test]
#[allow(clippy::diverging_sub_expression)]
fn early_exit_drops_a_bad_body() {
    let result = html! {
        @for _ in 0..1 {
            style {
                (TrustedStyle::assume_trusted("</style><img src=x onerror=alert(1)>"))
                @let _x: () = { break; };
            }
        }
    };
    assert_eq!(result.into_string(), "<style></style>");
}

/// The compile-time check is cheap enough for a vendored library.
#[test]
fn large_trusted_constant() {
    const LEN: usize = 200_000;
    #[allow(clippy::large_const_arrays)]
    const BYTES: [u8; LEN] = {
        let mut bytes = [b'a'; LEN];
        let mut i = 0;
        while i < LEN {
            // A `<` that is not the start of `<script` every few bytes, the
            // slow path of the scan.
            if i % 7 == 0 {
                bytes[i] = b'<';
            }
            i += 1;
        }
        bytes
    };
    const BIG: &str = match std::str::from_utf8(&BYTES) {
        Ok(s) => s,
        Err(_) => panic!(),
    };
    let script = trusted_script!(BIG);
    let style = trusted_style!(BIG);
    assert_eq!(script.as_str().len(), LEN);
    assert_eq!(style.as_str().len(), LEN);
}

/// The macros define no named items, so the caller's constants can have any
/// name.
#[test]
fn trusted_macros_accept_any_const_name() {
    const __MAUD_TRUSTED_SCRIPT: &str = "a();";
    const __MAUD_TRUSTED_STYLE: &str = "p {}";
    const CODE: &str = "b();";
    let result = html! {
        script { (trusted_script!(__MAUD_TRUSTED_SCRIPT)) (trusted_script!(CODE)) }
        style { (trusted_style!(__MAUD_TRUSTED_STYLE)) }
    };
    assert_eq!(
        result.into_string(),
        "<script>a();b();</script><style>p {}</style>"
    );
}

/// Attribute names written as string literals still work when they are
/// plain names.
#[test]
fn string_literal_attribute_names() {
    let result = html! { script "data-x"="1" "@load"="go()" type="module" {} };
    assert_eq!(
        result.into_string(),
        r#"<script data-x="1" @load="go()" type="module"></script>"#
    );
}
