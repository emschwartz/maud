#![no_std]

//! A macro for writing HTML templates.
//!
//! This documentation only describes the runtime API. For a general
//! guide, check out the [book] instead.
//!
//! [book]: https://maud.lambda.xyz/

#![doc(html_root_url = "https://docs.rs/maud/0.27.0")]

extern crate alloc;

use alloc::{borrow::Cow, boxed::Box, string::String};
use core::fmt::{self, Arguments, Display, Write};

pub use maud_macros::html;

mod escape;

pub mod ctx;

pub use ctx::{RenderIn, TrustedScript, TrustedStyle};

#[cfg(feature = "json")]
extern crate std;

#[cfg(feature = "json")]
pub mod json;

/// An adapter that escapes HTML special characters.
///
/// The following characters are escaped:
///
/// * `&` is escaped as `&amp;`
/// * `<` is escaped as `&lt;`
/// * `>` is escaped as `&gt;`
/// * `"` is escaped as `&quot;`
///
/// All other characters are passed through unchanged.
///
/// **Note:** In versions prior to 0.13, the single quote (`'`) was
/// escaped as well.
///
/// # Example
///
/// ```rust
/// use maud::Escaper;
/// use std::fmt::Write;
/// let mut s = String::new();
/// write!(Escaper::new(&mut s), "<script>launchMissiles()</script>").unwrap();
/// assert_eq!(s, "&lt;script&gt;launchMissiles()&lt;/script&gt;");
/// ```
pub struct Escaper<'a>(&'a mut String);

impl<'a> Escaper<'a> {
    /// Creates an `Escaper` from a `String`.
    pub fn new(buffer: &'a mut String) -> Escaper<'a> {
        Escaper(buffer)
    }
}

impl fmt::Write for Escaper<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        escape::escape_to_string(s, self.0);
        Ok(())
    }
}

/// Represents a type that can be rendered as HTML.
///
/// To implement this for your own type, override either the `.render()`
/// or `.render_to()` methods; since each is defined in terms of the
/// other, you only need to implement one of them. See the example below.
///
/// # Minimal implementation
///
/// An implementation of this trait must override at least one of
/// `.render()` or `.render_to()`. Since the default definitions of
/// these methods call each other, not doing this will result in
/// infinite recursion.
///
/// # Example
///
/// ```rust
/// use maud::{html, Markup, Render};
///
/// /// Provides a shorthand for linking to a CSS stylesheet.
/// pub struct Stylesheet(&'static str);
///
/// impl Render for Stylesheet {
///     fn render(&self) -> Markup {
///         html! {
///             link rel="stylesheet" type="text/css" href=(self.0);
///         }
///     }
/// }
/// ```
pub trait Render {
    /// Renders `self` as a block of `Markup`.
    fn render(&self) -> Markup {
        let mut buffer = String::new();
        self.render_to(&mut buffer);
        PreEscaped(buffer)
    }

    /// Appends a representation of `self` to the given buffer.
    ///
    /// Its default implementation just calls `.render()`, but you may
    /// override it with something more efficient.
    ///
    /// Note that no further escaping is performed on data written to
    /// the buffer. If you override this method, you must make sure that
    /// any data written is properly escaped, whether by hand or using
    /// the [`Escaper`](struct.Escaper.html) wrapper struct.
    fn render_to(&self, buffer: &mut String) {
        buffer.push_str(&self.render().into_string());
    }
}

impl Render for str {
    fn render_to(&self, w: &mut String) {
        escape::escape_to_string(self, w);
    }
}

impl Render for String {
    fn render_to(&self, w: &mut String) {
        str::render_to(self, w);
    }
}

impl Render for Cow<'_, str> {
    fn render_to(&self, w: &mut String) {
        str::render_to(self, w);
    }
}

impl Render for Arguments<'_> {
    fn render_to(&self, w: &mut String) {
        let _ = Escaper::new(w).write_fmt(*self);
    }
}

impl<T: Render + ?Sized> Render for &T {
    fn render_to(&self, w: &mut String) {
        T::render_to(self, w);
    }
}

impl<T: Render + ?Sized> Render for &mut T {
    fn render_to(&self, w: &mut String) {
        T::render_to(self, w);
    }
}

impl<T: Render + ?Sized> Render for Box<T> {
    fn render_to(&self, w: &mut String) {
        T::render_to(self, w);
    }
}

#[cfg(target_has_atomic = "ptr")]
impl<T: Render + ?Sized> Render for alloc::sync::Arc<T> {
    fn render_to(&self, w: &mut String) {
        T::render_to(self, w);
    }
}

macro_rules! impl_render_with_display {
    ($($ty:ty)*) => {
        $(
            impl Render for $ty {
                fn render_to(&self, w: &mut String) {
                    format_args!("{self}").render_to(w);
                }
            }
        )*
    };
}

impl_render_with_display! {
    char f32 f64
}

macro_rules! impl_render_with_itoa {
    ($($ty:ty)*) => {
        $(
            impl Render for $ty {
                fn render_to(&self, w: &mut String) {
                    w.push_str(itoa::Buffer::new().format(*self));
                }
            }
        )*
    };
}

impl_render_with_itoa! {
    i8 i16 i32 i64 i128 isize
    u8 u16 u32 u64 u128 usize
}

/// Renders a value using its [`Display`] impl.
///
/// # Example
///
/// ```rust
/// use maud::html;
/// use std::net::Ipv4Addr;
///
/// let ip_address = Ipv4Addr::new(127, 0, 0, 1);
///
/// let markup = html! {
///     "My IP address is: "
///     (maud::display(ip_address))
/// };
///
/// assert_eq!(markup.into_string(), "My IP address is: 127.0.0.1");
/// ```
pub fn display(value: impl Display) -> impl Render {
    struct DisplayWrapper<T>(T);

    impl<T: Display> Render for DisplayWrapper<T> {
        fn render_to(&self, w: &mut String) {
            format_args!("{0}", self.0).render_to(w);
        }
    }

    DisplayWrapper(value)
}

/// A wrapper that renders the inner value without escaping.
#[derive(Debug, Clone, Copy)]
pub struct PreEscaped<T>(pub T);

impl<T: AsRef<str>> Render for PreEscaped<T> {
    fn render_to(&self, w: &mut String) {
        w.push_str(self.0.as_ref());
    }
}

/// A block of markup is a string that does not need to be escaped.
///
/// The `html!` macro expands to an expression of this type.
pub type Markup = PreEscaped<String>;

impl<T: Into<String>> PreEscaped<T> {
    /// Converts the inner value to a string.
    pub fn into_string(self) -> String {
        self.0.into()
    }
}

impl<T: Into<String>> From<PreEscaped<T>> for String {
    fn from(value: PreEscaped<T>) -> String {
        value.into_string()
    }
}

impl<T: Default> Default for PreEscaped<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

/// The literal string `<!DOCTYPE html>`.
///
/// # Example
///
/// A minimal web page:
///
/// ```rust
/// use maud::{DOCTYPE, html};
///
/// let markup = html! {
///     (DOCTYPE)
///     html {
///         head {
///             meta charset="utf-8";
///             title { "Test page" }
///         }
///         body {
///             p { "Hello, world!" }
///         }
///     }
/// };
/// ```
pub const DOCTYPE: PreEscaped<&'static str> = PreEscaped("<!DOCTYPE html>");

#[cfg(feature = "rocket")]
mod rocket_support {
    extern crate std;

    use crate::PreEscaped;
    use alloc::string::String;
    use rocket::{
        http::ContentType,
        request::Request,
        response::{Responder, Response},
    };
    use std::io::Cursor;

    impl Responder<'_, 'static> for PreEscaped<String> {
        fn respond_to(self, _: &Request) -> rocket::response::Result<'static> {
            Response::build()
                .header(ContentType::HTML)
                .sized_body(self.0.len(), Cursor::new(self.0))
                .ok()
        }
    }
}

#[cfg(feature = "actix-web")]
mod actix_support {
    use core::{
        pin::Pin,
        task::{Context, Poll},
    };

    use crate::PreEscaped;
    use actix_web_dep::{
        HttpRequest, HttpResponse, Responder,
        body::{BodySize, MessageBody},
        http::header,
        web::Bytes,
    };
    use alloc::string::String;

    impl MessageBody for PreEscaped<String> {
        type Error = <String as MessageBody>::Error;

        fn size(&self) -> BodySize {
            self.0.size()
        }

        fn poll_next(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Bytes, Self::Error>>> {
            Pin::new(&mut self.0).poll_next(cx)
        }
    }

    impl Responder for PreEscaped<String> {
        type Body = String;

        fn respond_to(self, _req: &HttpRequest) -> HttpResponse<Self::Body> {
            HttpResponse::Ok()
                .content_type(header::ContentType::html())
                .message_body(self.0)
                .unwrap()
        }
    }
}

#[cfg(feature = "poem")]
mod poem_support {
    use crate::PreEscaped;
    use alloc::string::String;
    use poem::{IntoResponse, Response, web::Html};

    impl IntoResponse for PreEscaped<String> {
        fn into_response(self) -> Response {
            Html(self.into_string()).into_response()
        }
    }
}

#[cfg(feature = "axum")]
mod axum_support {
    use crate::PreEscaped;
    use alloc::string::String;
    use axum_core::response::{IntoResponse, Response};
    use http::{HeaderValue, header};

    impl IntoResponse for PreEscaped<String> {
        fn into_response(self) -> Response {
            let headers = [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )];
            (headers, self.0).into_response()
        }
    }
}

#[cfg(feature = "warp")]
mod warp_support {
    use crate::PreEscaped;
    use alloc::string::String;
    use warp::reply::{self, Reply, Response};

    impl Reply for PreEscaped<String> {
        fn into_response(self) -> Response {
            reply::html(self.into_string()).into_response()
        }
    }
}

#[cfg(feature = "submillisecond")]
mod submillisecond_support {
    use crate::PreEscaped;
    use alloc::string::String;
    use submillisecond::{
        http::{HeaderMap, HeaderValue, header},
        response::{IntoResponse, Response},
    };

    impl IntoResponse for PreEscaped<String> {
        fn into_response(self) -> Response {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            (headers, self.0).into_response()
        }
    }
}

#[cfg(feature = "salvo")]
mod salvo_support {
    use crate::PreEscaped;
    use alloc::string::String;
    use http::{HeaderValue, header::CONTENT_TYPE};
    use salvo_core::{http::Response, writing::Scribe};

    impl Scribe for PreEscaped<String> {
        fn render(self, res: &mut Response) {
            let _ = res.headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            let _ = res.write_body(self.into_string());
        }
    }
}

#[doc(hidden)]
pub mod macro_private {
    use crate::{
        Render, TrustedScript, TrustedStyle,
        ctx::{RenderIn, SCRIPT_FORBIDDEN, STYLE_FORBIDDEN, find_forbidden},
        display,
    };
    use alloc::string::String;
    use core::fmt::Display;

    /// Renders a splice inside a `<script>` or `<style>` body.
    ///
    /// Unlike `render_to!`, this has no `Display` fallback.
    pub fn render_in<C, T: RenderIn<C> + ?Sized>(value: &T, buffer: &mut String) {
        value.render_in(buffer);
    }

    /// Writes the end of a `<script>` or `<style>` element whose body has
    /// splices or control flow, after checking the body.
    ///
    /// The body is generated through [`RawText::buffer`], so that Rust
    /// control flow that leaves it early (`break`, `continue`, `return`, `?`)
    /// still closes the element, through `Drop`.
    pub struct RawText<'a> {
        buffer: &'a mut String,
        body_start: usize,
        forbidden: &'static [&'static str],
        element: &'static str,
        end_tag: &'static str,
    }

    impl<'a> RawText<'a> {
        /// Starts the body of a `<script>`, whose end tag is `end_tag`.
        pub fn script(buffer: &'a mut String, end_tag: &'static str) -> Self {
            Self::new(buffer, SCRIPT_FORBIDDEN, "script", end_tag)
        }

        /// Starts the body of a `<style>`, whose end tag is `end_tag`.
        pub fn style(buffer: &'a mut String, end_tag: &'static str) -> Self {
            Self::new(buffer, STYLE_FORBIDDEN, "style", end_tag)
        }

        fn new(
            buffer: &'a mut String,
            forbidden: &'static [&'static str],
            element: &'static str,
            end_tag: &'static str,
        ) -> Self {
            let body_start = buffer.len();
            Self {
                buffer,
                body_start,
                forbidden,
                element,
                end_tag,
            }
        }

        /// The buffer to write the body to.
        pub fn buffer(&mut self) -> &mut String {
            self.buffer
        }

        /// Ends the body normally. Panics if it would end the element early.
        #[track_caller]
        pub fn finish(self) {
            let Some(body) = self.buffer.get(self.body_start..) else {
                panic!(
                    "the buffer was truncated before the start of a `<{}>` body",
                    self.element
                );
            };
            if let Some((needle, offset)) = find_forbidden(body.as_bytes(), self.forbidden) {
                // Remove the body first, so that `Drop` does not find it again
                // while this panic unwinds.
                let found = String::from(&body[offset..offset + self.forbidden[needle].len()]);
                self.buffer.truncate(self.body_start);
                panic!(
                    "a `<{}>` body contains `{found}` at byte {offset}, which would end \
                     the element early or hide its end tag",
                    self.element,
                );
            }
            // `Drop` writes the end tag.
        }
    }

    impl Drop for RawText<'_> {
        /// Writes the end tag. When the body was left early, check it here
        /// too, but without panicking: this may run while a panic unwinds,
        /// and the crate has no `std::thread::panicking` to tell. A body that
        /// fails the check is dropped instead.
        fn drop(&mut self) {
            match self.buffer.get(self.body_start..) {
                Some(body) if find_forbidden(body.as_bytes(), self.forbidden).is_some() => {
                    self.buffer.truncate(self.body_start);
                }
                // A `Render` impl truncated the buffer: `finish` panics, or
                // the body was left early. Either way, there is no body left.
                _ => {}
            }
            self.buffer.push_str(self.end_tag);
        }
    }

    /// Fails const evaluation if `code` would end a `<script>` element early.
    pub const fn check_script_const(code: &str) {
        if find_forbidden(code.as_bytes(), SCRIPT_FORBIDDEN).is_some() {
            panic!(
                "a trusted script contains `</script` or `<script`, which would end \
                 the element early or hide its end tag"
            );
        }
    }

    /// Fails const evaluation if `code` would end a `<style>` element early.
    pub const fn check_style_const(code: &str) {
        if find_forbidden(code.as_bytes(), STYLE_FORBIDDEN).is_some() {
            panic!("a trusted style contains `</style`, which would end the element early");
        }
    }

    /// Wraps a constant for `trusted_script!`, which checks it separately.
    ///
    /// Anything a macro expands to can also be written by hand, so this
    /// cannot be hidden from other crates. It is named like the escape hatch
    /// so that a search for `assume_trusted` finds it, and like
    /// `TrustedScript::assume_trusted` it is still covered by the check that
    /// runs when the enclosing `<script>` closes.
    pub const fn assume_trusted_script_const(code: &'static str) -> TrustedScript {
        TrustedScript::from_static(code)
    }

    /// Wraps a constant for `trusted_style!`. See `assume_trusted_script_const`.
    pub const fn assume_trusted_style_const(code: &'static str) -> TrustedStyle {
        TrustedStyle::from_static(code)
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! render_to {
        ($x:expr, $buffer:expr) => {{
            use $crate::macro_private::*;
            match ChooseRenderOrDisplay($x) {
                x => (&&x).implements_render_or_display().render_to(x.0, $buffer),
            }
        }};
    }

    pub use render_to;

    pub struct ChooseRenderOrDisplay<T>(pub T);

    pub struct ViaRenderTag;
    pub struct ViaDisplayTag;

    pub trait ViaRender {
        fn implements_render_or_display(&self) -> ViaRenderTag {
            ViaRenderTag
        }
    }
    pub trait ViaDisplay {
        fn implements_render_or_display(&self) -> ViaDisplayTag {
            ViaDisplayTag
        }
    }

    impl<T: Render> ViaRender for &ChooseRenderOrDisplay<T> {}
    impl<T: Display> ViaDisplay for ChooseRenderOrDisplay<T> {}

    impl ViaRenderTag {
        pub fn render_to<T: Render + ?Sized>(self, value: &T, buffer: &mut String) {
            value.render_to(buffer);
        }
    }

    impl ViaDisplayTag {
        pub fn render_to<T: Display + ?Sized>(self, value: &T, buffer: &mut String) {
            display(value).render_to(buffer);
        }
    }
}
