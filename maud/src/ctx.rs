//! Rendering into `<script>` and `<style>` bodies.
//!
//! The browser does not decode character references inside `<script>` and
//! `<style>`, so the HTML escaping that [`Render`](crate::Render) performs is
//! wrong there: it breaks valid code (`a &lt; b`) and, since it leaves `'`
//! alone, does not stop injection into a JavaScript string either.
//!
//! Instead, `html!` picks a context from the enclosing element at compile time,
//! and a splice in that context must implement [`RenderIn`] for it:
//!
//! | Element | Context | Splices accepted |
//! |---|---|---|
//! | `script type="application/json"` or `"application/ld+json"` | [`JsonScript`] | [`Json`], [`serde_json::Value`], [`serde_json::value::RawValue`] |
//! | `script type="text/template"` or `"text/x-template"` | HTML, as anywhere else | [`Render`](crate::Render) |
//! | any other `script`, including no `type`, `module`, or a dynamic `type` | [`Script`] | [`TrustedScript`] |
//! | `style` | [`Style`] | [`TrustedStyle`] |
//!
//! String literals are emitted unescaped in every row except the template one.
//!
//! When a `script` or `style` element closes, its body is checked for text that
//! would end the element early. Literals are checked at compile time; a body
//! with splices or control flow is checked again at run time, and the check
//! panics on a hit. If Rust control flow leaves the body early (`break`,
//! `continue`, `return` or `?`), the element is still closed, and a body that
//! fails the check is dropped instead of panicking.
//!
//! These rules keep untrusted data out of the page's markup and scripts
//! wherever the element ends up, including when the browser parses it as an
//! SVG or MathML element (where it decodes character references): untrusted
//! data never reaches an executable body, JSON output contains no `<`, `>` or
//! `&`, and escaped template text contains no `<`. Still, `script` and `style`
//! inside `svg`, `math`, `textarea`, `title`, `xmp`, `iframe`, `noembed`,
//! `noframes`, `plaintext` or `frameset` in the same `html!` call are a compile
//! error, since how they parse depends on that parent.
//!
//! # Limitation: template scripts in foreign content
//!
//! A `script type="text/template"` built in one `html!` call and spliced into
//! an `svg` or `math` element in another is parsed as a foreign element, so
//! the browser decodes the escaped text. The page itself is still safe, but the
//! template's `textContent` then holds the spliced markup as written, and a
//! framework that instantiates the template with `innerHTML` builds it. `html!`
//! cannot see across calls, so keep template scripts out of `svg` and `math`.
//!
//! [`Json`]: crate::json::Json
//!
//! # Passing data to a script
//!
//! Put the data in a JSON block, and read it from the script:
//!
//! ```rust
//! # #[cfg(feature = "json")] {
//! use maud::{html, json::Json};
//!
//! let user = serde_json::json!({ "name": "</script>" });
//! let page = html! {
//!     script type="application/json" id="user" { (Json(&user)) }
//!     script { "const user = JSON.parse(document.getElementById('user').textContent);" }
//! };
//! # }
//! ```

use alloc::{borrow::Cow, string::String};

/// The body of an executable `<script>` element.
pub enum Script {}

/// The body of a `<style>` element.
pub enum Style {}

/// The body of a `<script type="application/json">` or
/// `<script type="application/ld+json">` element.
pub enum JsonScript {}

pub(crate) mod sealed {
    pub trait Sealed<C> {}
}

/// Represents a type that can be spliced into the body of a `<script>` or
/// `<style>` element.
///
/// `C` is one of the context markers in [`ctx`](self). This trait is sealed:
/// the set of types that are safe to splice into raw text is fixed by `maud`.
/// See the [module docs](self) for which types each context accepts.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be spliced into this element body (`{C}`)",
    label = "not allowed in this `<script>` or `<style>` body",
    note = "an executable `<script>` only accepts `TrustedScript`, and a `<style>` only accepts `TrustedStyle`",
    note = "to pass data to a script, put it in `script type=\"application/json\" {{ (Json(..)) }}` and read it with `JSON.parse(element.textContent)`"
)]
pub trait RenderIn<C>: sealed::Sealed<C> {
    /// Appends `self` to the given buffer, in context `C`.
    fn render_in(&self, buffer: &mut String);
}

macro_rules! impl_render_in_for_pointer {
    ($($(#[$meta:meta])* $ty:ty;)*) => {
        $(
            $(#[$meta])*
            impl<C, T: RenderIn<C> + ?Sized> sealed::Sealed<C> for $ty {}

            $(#[$meta])*
            impl<C, T: RenderIn<C> + ?Sized> RenderIn<C> for $ty {
                fn render_in(&self, buffer: &mut String) {
                    T::render_in(self, buffer);
                }
            }
        )*
    };
}

// Keep in sync with the pointer impls of `Render`.
impl_render_in_for_pointer! {
    &T;
    &mut T;
    alloc::boxed::Box<T>;
    #[cfg(target_has_atomic = "ptr")]
    alloc::sync::Arc<T>;
}

macro_rules! trusted_type {
    (
        $(#[$meta:meta])*
        $name:ident, $ctx:ident, $element:literal
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone)]
        pub struct $name(Cow<'static, str>);

        impl $name {
            /// Wraps code that is trusted without any check.
            ///
            /// This is the escape hatch for code that is not a constant, such as
            /// code read from a file at run time. Never pass it text that came
            /// from a user.
            ///
            #[doc = concat!(
                "The body of the enclosing `", $element, "` element is still ",
                "checked when it closes, so this panics there if the code would ",
                "end the element early."
            )]
            pub fn assume_trusted(code: impl Into<Cow<'static, str>>) -> Self {
                Self(code.into())
            }

            /// Returns the wrapped code.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Wraps a constant without checking it. Only reachable through
            /// `macro_private`, for the `trusted_*!` macros.
            pub(crate) const fn from_static(code: &'static str) -> Self {
                Self(Cow::Borrowed(code))
            }
        }

        impl sealed::Sealed<$ctx> for $name {}

        impl RenderIn<$ctx> for $name {
            fn render_in(&self, buffer: &mut String) {
                buffer.push_str(&self.0);
            }
        }
    };
}

trusted_type! {
    /// JavaScript that is trusted to be spliced into an executable `<script>`.
    ///
    /// Build one from a constant with [`trusted_script!`](crate::trusted_script),
    /// or, as a last resort, with [`assume_trusted`](Self::assume_trusted).
    TrustedScript, Script, "script"
}

trusted_type! {
    /// CSS that is trusted to be spliced into a `<style>`.
    ///
    /// Build one from a constant with [`trusted_style!`](crate::trusted_style),
    /// or, as a last resort, with [`assume_trusted`](Self::assume_trusted).
    TrustedStyle, Style, "style"
}

/// Builds a [`TrustedScript`] from a constant string.
///
/// The argument must be a constant expression, such as a literal, a `const`,
/// `concat!(..)` or `include_str!(..)`. It is checked at compile time for text
/// that would end the `<script>` element early.
///
/// # Example
///
/// ```rust
/// use maud::{html, trusted_script};
///
/// const GREETING: &str = "console.log('hi');";
/// let markup = html! {
///     script { (trusted_script!(GREETING)) }
/// };
/// assert_eq!(markup.into_string(), "<script>console.log('hi');</script>");
/// ```
///
/// A value that is not a constant is rejected:
///
/// ```rust,compile_fail
/// let code = String::from("alert(1)");
/// let _ = maud::trusted_script!(&code);
/// ```
#[macro_export]
macro_rules! trusted_script {
    ($code:expr $(,)?) => {{
        // An anonymous `const _` item is always evaluated, by `cargo check`
        // and in dead code too, unlike an inline `const { .. }` block. And
        // having no name, it cannot shadow a constant that `$code` names.
        #[allow(long_running_const_eval)]
        const _: () = $crate::macro_private::check_script_const($code);
        const { $crate::macro_private::assume_trusted_script_const($code) }
    }};
}

/// Builds a [`TrustedStyle`] from a constant string.
///
/// See [`trusted_script!`](crate::trusted_script) for the rules.
#[macro_export]
macro_rules! trusted_style {
    ($code:expr $(,)?) => {{
        #[allow(long_running_const_eval)]
        const _: () = $crate::macro_private::check_style_const($code);
        const { $crate::macro_private::assume_trusted_style_const($code) }
    }};
}

// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
// !!!!! PLEASE KEEP THESE IN SYNC WITH `maud_macros/src/raw_text.rs` !!!!!!!!!!
// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

/// Sequences that may not appear in a `<script>` body, lowercase.
///
/// This follows the WHATWG ["restrictions for contents of `script`
/// elements"][restrictions]. `</script` ends the element. `<script` matters
/// only after `<!--`, where it moves the tokenizer into the "script data double
/// escaped" state; from there a `</script>` no longer ends the element, and the
/// rest of the page is swallowed as script.
///
/// `<!--` alone is allowed. The only way out of the "escaped" state it starts
/// is through `<script` (rejected here) or `</script` (also rejected), so
/// without them `<!--` cannot change where the element ends. Rejecting it too
/// would break code that the specification allows.
///
/// [restrictions]: https://html.spec.whatwg.org/multipage/scripting.html#restrictions-for-contents-of-script-elements
pub(crate) const SCRIPT_FORBIDDEN: &[&str] = &["</script", "<script"];

/// Sequences that may not appear in a `<style>` body, lowercase.
///
/// `<style>` is a raw text element without the comment states of `<script>`,
/// so only its end tag matters.
pub(crate) const STYLE_FORBIDDEN: &[&str] = &["</style"];

/// Finds the first of `needles` (lowercase ASCII, each starting with `<`) in
/// `haystack`, ignoring ASCII case. Returns the needle's index and the byte
/// offset of the match.
///
/// This also runs in const evaluation over whole `include_str!` files, so it
/// skips quickly to the next `<` and only compares there.
pub(crate) const fn find_forbidden(haystack: &[u8], needles: &[&str]) -> Option<(usize, usize)> {
    let mut start = 0;
    while start < haystack.len() {
        if haystack[start] == b'<' {
            let mut n = 0;
            while n < needles.len() {
                let needle = needles[n].as_bytes();
                if start + needle.len() <= haystack.len() {
                    let mut i = 1;
                    while i < needle.len() && haystack[start + i].to_ascii_lowercase() == needle[i]
                    {
                        i += 1;
                    }
                    if i == needle.len() {
                        return Some((n, start));
                    }
                }
                n += 1;
            }
        }
        start += 1;
    }
    None
}
