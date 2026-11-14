use std::cell::RefCell;

use proc_macro2::{Ident, Span, TokenStream};
use proc_macro2_diagnostics::{Diagnostic, SpanDiagnosticExt};
use quote::{ToTokens, quote, quote_spanned};
use syn::{Expr, Local, parse_quote, spanned::Spanned, token::Brace};

use crate::{
    ast::*,
    escape,
    raw_text::{self, ScriptKind},
};

pub fn generate(
    markups: Markups<Element>,
    output_ident: Ident,
    diagnostics: &mut Vec<Diagnostic>,
) -> TokenStream {
    let mut build = Builder::new(output_ident.clone());
    let generator = Generator::new(output_ident);
    generator.markups(markups, Ctx::HTML, &mut build);
    diagnostics.extend(generator.diagnostics.into_inner());
    build.finish()
}

/// What kind of content the markups being generated are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Body {
    /// Ordinary HTML text: literals and splices are HTML-escaped.
    Normal,
    /// The body of a `<script>` element.
    Script(ScriptKind),
    /// The body of a `<style>` element.
    Style,
}

#[derive(Debug, Clone, Copy)]
struct Ctx {
    body: Body,
    /// The nearest enclosing element, in this `html!` call, that changes how
    /// a `script` or `style` inside it is parsed. Only used to reject them
    /// there.
    unsupported_parent: Option<&'static str>,
}

impl Ctx {
    const HTML: Ctx = Ctx {
        body: Body::Normal,
        unsupported_parent: None,
    };

    /// Whether this is the body of a `script` or `style` element.
    fn is_raw_text(self) -> bool {
        self.body != Body::Normal
    }

    /// Whether string literals are HTML-escaped.
    fn escapes_literals(self) -> bool {
        matches!(self.body, Body::Normal | Body::Script(ScriptKind::Template))
    }
}

struct Generator {
    output_ident: Ident,
    diagnostics: RefCell<Vec<Diagnostic>>,
}

impl Generator {
    fn new(output_ident: Ident) -> Generator {
        Generator {
            output_ident,
            diagnostics: RefCell::new(Vec::new()),
        }
    }

    fn error(&self, diagnostic: Diagnostic) {
        self.diagnostics.borrow_mut().push(diagnostic);
    }

    fn builder(&self) -> Builder {
        Builder::new(self.output_ident.clone())
    }

    fn markups<E: Into<Element>>(&self, markups: Markups<E>, ctx: Ctx, build: &mut Builder) {
        for markup in markups.markups {
            self.markup(markup, ctx, build);
        }
    }

    fn markup<E: Into<Element>>(&self, markup: Markup<E>, ctx: Ctx, build: &mut Builder) {
        match markup {
            Markup::Block(block) => {
                if block.markups.markups.iter().any(|markup| {
                    matches!(
                        *markup,
                        Markup::ControlFlow(ControlFlow {
                            kind: ControlFlowKind::Let(_),
                            ..
                        })
                    )
                }) {
                    self.block(block, ctx, build);
                } else {
                    self.markups(block.markups, ctx, build);
                }
            }
            Markup::Lit(lit) => {
                if ctx.escapes_literals() {
                    build.push_escaped(&lit.to_string());
                } else {
                    build.push_str(&lit.to_string());
                }
            }
            Markup::Splice { expr, .. } => self.splice(expr, ctx, build),
            Markup::Element(element) => {
                let element = element.into();
                if ctx.is_raw_text() {
                    let parent = match ctx.body {
                        Body::Style => "style",
                        _ => "script",
                    };
                    self.error(
                        element
                            .span()
                            .error(format!("elements are not allowed inside `{parent}`"))
                            .note(format!(
                                "the browser reads the body of `{parent}` as plain text, not HTML"
                            )),
                    );
                } else {
                    self.element(element, ctx, build);
                }
            }
            Markup::ControlFlow(control_flow) => self.control_flow(control_flow, ctx, build),
            Markup::Semi(_) => {}
        }
    }

    fn block<E: Into<Element>>(&self, block: Block<E>, ctx: Ctx, build: &mut Builder) {
        let markups = {
            let mut build = self.builder();
            self.markups(block.markups, ctx, &mut build);
            build.finish()
        };

        build.push_tokens(quote!({ #markups }));
    }

    fn splice(&self, expr: Expr, ctx: Ctx, build: &mut Builder) {
        let output_ident = &self.output_ident;
        let context = match ctx.body {
            Body::Normal | Body::Script(ScriptKind::Template) => {
                build.push_tokens(
                    quote!(maud::macro_private::render_to!(&(#expr), &mut #output_ident);),
                );
                return;
            }
            Body::Script(ScriptKind::Executable) => quote!(Script),
            Body::Script(ScriptKind::Json) => quote!(JsonScript),
            Body::Style => quote!(Style),
        };
        // Span the call on the splice, so that a type that does not implement
        // `RenderIn` is reported there.
        build.push_tokens(quote_spanned! {expr.span()=>
            maud::macro_private::render_in::<maud::ctx::#context, _>(&(#expr), &mut #output_ident);
        });
    }

    fn element(&self, element: Element, parent: Ctx, build: &mut Builder) {
        let element_name = element.name.clone().unwrap_or_else(|| parse_quote!(div));
        let lower_name = element_name.to_string().to_ascii_lowercase();
        let ctx = Ctx {
            body: match lower_name.as_str() {
                "script" => Body::Script(script_kind(&element.attrs)),
                "style" => Body::Style,
                _ => Body::Normal,
            },
            unsupported_parent: raw_text::UNSUPPORTED_PARENTS
                .iter()
                .find(|parent| **parent == lower_name)
                .copied()
                .or(parent.unsupported_parent),
        };

        // A usability guard, not a safety mechanism: the rules above keep
        // untrusted data out of executable code wherever the element ends up.
        // But inside these parents the browser parses a `script` or `style`
        // differently (as a foreign element, or as plain text whose literals
        // could close the parent), so the output is hard to predict.
        if let (true, Some(unsupported)) = (ctx.is_raw_text(), parent.unsupported_parent) {
            self.error(
                element_name
                    .span()
                    .error(format!(
                        "`{lower_name}` is not supported inside `{unsupported}`"
                    ))
                    .help(format!(
                        "move the `{lower_name}` outside the `{unsupported}` element"
                    )),
            );
        }

        for attr in &element.attrs {
            if let Attribute::Named { name, .. } = attr {
                let text = name.to_string();
                if let Some(bad) = text
                    .chars()
                    .find(|c| raw_text::is_invalid_in_attribute_name(*c))
                {
                    self.error(
                        name.span()
                            .error(format!("attribute name {text:?} contains {bad:?}"))
                            .note("the browser would split this name into several attributes"),
                    );
                }
            }
        }

        if let (true, ElementBody::Void(semi)) = (ctx.is_raw_text(), &element.body) {
            self.error(
                semi.span()
                    .error(format!("`{lower_name}` must have a body"))
                    .help("replace the `;` with `{}`")
                    .note(format!(
                        "without a closing `</{lower_name}>` tag, the browser reads the rest of the page as part of the `{lower_name}`"
                    )),
            );
        }

        build.push_str("<");
        self.name(element_name.clone(), build);
        self.attrs(element.attrs, build);
        build.push_str(">");
        if let ElementBody::Block(block) = element.body {
            let dynamic = ctx.is_raw_text() && self.raw_text_body_checks(&block.markups, ctx);
            if dynamic {
                // Generate the body through a guard that checks it and writes
                // the end tag, even if Rust control flow leaves it early.
                let output_ident = &self.output_ident;
                let guard = Ident::new("__maud_raw_text", Span::mixed_site());
                let constructor = match ctx.body {
                    Body::Style => quote!(style),
                    _ => quote!(script),
                };
                let end_tag = format!("</{element_name}>");
                let body = {
                    let mut build = self.builder();
                    self.markups(block.markups, ctx, &mut build);
                    build.finish()
                };
                build.push_tokens(quote!({
                    let mut #guard = maud::macro_private::RawText::#constructor(
                        &mut #output_ident,
                        #end_tag,
                    );
                    {
                        // Splices take `&mut #output_ident`, which needs `mut`.
                        #[allow(unused_mut)]
                        let mut #output_ident: &mut alloc::string::String = #guard.buffer();
                        #body
                    }
                    // Unreachable when the body always leaves early; `Drop`
                    // closes the element then.
                    #[allow(unreachable_code)]
                    #guard.finish();
                }));
            } else {
                self.markups(block.markups, ctx, build);
                build.push_str("</");
                self.name(element_name, build);
                build.push_str(">");
            }
        }
    }

    /// Checks the string literals in a `script` or `style` body at compile
    /// time. Returns whether the body also needs a check at run time.
    fn raw_text_body_checks(&self, markups: &Markups<Element>, ctx: Ctx) -> bool {
        let (forbidden, element) = match ctx.body {
            Body::Normal => return false,
            Body::Style => (raw_text::STYLE_FORBIDDEN, "style"),
            Body::Script(_) => (raw_text::SCRIPT_FORBIDDEN, "script"),
        };

        // Escaped literals cannot contain `<`, so there is nothing to check
        // at compile time. A splice can still emit anything.
        if !ctx.escapes_literals() {
            let mut runs = Vec::new();
            let mut current = Run::default();
            static_runs(&markups.markups, &mut current, &mut runs);
            runs.push(current);

            for run in runs {
                for (needle, offset) in raw_text::find_forbidden(&run.text, forbidden) {
                    let first = run.literal_at(offset);
                    let last = run.literal_at(offset + needle.len() - 1);
                    let (first, last, split) = (run.span(first), run.span(last), first != last);
                    let note = if needle == "<script" {
                        "after `<!--`, it stops the browser from finding the closing `</script>` tag"
                            .to_string()
                    } else {
                        format!("the browser would end the `{element}` element here")
                    };
                    let mut diagnostic = first
                        .error(format!("`{needle}` cannot appear inside `{element}`"))
                        .note(note);
                    if split {
                        diagnostic = diagnostic.span_note(
                            last,
                            "the text continues in this literal; adjacent literals are joined",
                        );
                    }
                    self.error(diagnostic);
                }
            }
        }

        has_dynamic_content(&markups.markups)
    }

    fn name(&self, name: HtmlName, build: &mut Builder) {
        build.push_escaped(&name.to_string());
    }

    fn name_or_markup(&self, name: HtmlNameOrMarkup, build: &mut Builder) {
        match name {
            HtmlNameOrMarkup::HtmlName(name) => self.name(name, build),
            HtmlNameOrMarkup::Markup(markup) => self.markup(markup, Ctx::HTML, build),
        }
    }

    fn attr(&self, name: HtmlName, value: AttributeType, build: &mut Builder) {
        match value {
            AttributeType::Normal { value, .. } => {
                build.push_str(" ");
                self.name(name, build);
                build.push_str("=\"");
                self.markup(value, Ctx::HTML, build);
                build.push_str("\"");
            }
            AttributeType::Optional {
                toggler: Toggler { cond, .. },
                ..
            } => {
                let inner_value: Expr = parse_quote!(inner_value);

                let body = {
                    let mut build = self.builder();
                    build.push_str(" ");
                    self.name(name, &mut build);
                    build.push_str("=\"");
                    self.splice(inner_value.clone(), Ctx::HTML, &mut build);
                    build.push_str("\"");
                    build.finish()
                };
                build.push_tokens(quote!(if let Some(#inner_value) = (#cond) { #body }));
            }
            AttributeType::Empty(None) => {
                build.push_str(" ");
                self.name(name, build);
            }
            AttributeType::Empty(Some(Toggler { cond, .. })) => {
                let body = {
                    let mut build = self.builder();
                    build.push_str(" ");
                    self.name(name, &mut build);
                    build.finish()
                };
                build.push_tokens(quote!(if (#cond) { #body }));
            }
        }
    }

    fn attrs(&self, attrs: Vec<Attribute>, build: &mut Builder) {
        let (classes, id, named_attrs) = split_attrs(attrs);

        if !classes.is_empty() {
            let mut toggle_class_exprs = vec![];

            build.push_str(" ");
            self.name(parse_quote!(class), build);
            build.push_str("=\"");
            for (i, (name, toggler)) in classes.into_iter().enumerate() {
                if let Some(toggler) = toggler {
                    toggle_class_exprs.push((i > 0, name, toggler));
                } else {
                    if i > 0 {
                        build.push_str(" ");
                    }
                    self.name_or_markup(name, build);
                }
            }

            for (not_first, name, toggler) in toggle_class_exprs {
                let body = {
                    let mut build = self.builder();
                    if not_first {
                        build.push_str(" ");
                    }
                    self.name_or_markup(name, &mut build);
                    build.finish()
                };
                build.push_tokens(quote!(if (#toggler) { #body }));
            }

            build.push_str("\"");
        }

        if let Some(id) = id {
            build.push_str(" ");
            self.name(parse_quote!(id), build);
            build.push_str("=\"");
            self.name_or_markup(id, build);
            build.push_str("\"");
        }

        for (name, attr_type) in named_attrs {
            self.attr(name, attr_type, build);
        }
    }

    fn control_flow<E: Into<Element>>(
        &self,
        control_flow: ControlFlow<E>,
        ctx: Ctx,
        build: &mut Builder,
    ) {
        match control_flow.kind {
            ControlFlowKind::If(if_) => self.control_flow_if(if_, ctx, build),
            ControlFlowKind::Let(let_) => self.control_flow_let(let_, build),
            ControlFlowKind::For(for_) => self.control_flow_for(for_, ctx, build),
            ControlFlowKind::While(while_) => self.control_flow_while(while_, ctx, build),
            ControlFlowKind::Match(match_) => self.control_flow_match(match_, ctx, build),
        }
    }

    fn control_flow_if<E: Into<Element>>(
        &self,
        IfExpr {
            if_token,
            cond,
            then_branch,
            else_branch,
        }: IfExpr<E>,
        ctx: Ctx,
        build: &mut Builder,
    ) {
        build.push_tokens(quote!(#if_token #cond));
        self.block(then_branch, ctx, build);

        if let Some((_, else_token, if_or_block)) = else_branch {
            build.push_tokens(quote!(#else_token));
            self.control_flow_if_or_block(*if_or_block, ctx, build);
        }
    }

    fn control_flow_if_or_block<E: Into<Element>>(
        &self,
        if_or_block: IfOrBlock<E>,
        ctx: Ctx,
        build: &mut Builder,
    ) {
        match if_or_block {
            IfOrBlock::If(if_) => self.control_flow_if(if_, ctx, build),
            IfOrBlock::Block(block) => self.block(block, ctx, build),
        }
    }

    fn control_flow_let(&self, let_: Local, build: &mut Builder) {
        build.push_tokens(let_.to_token_stream());
    }

    fn control_flow_for<E: Into<Element>>(
        &self,
        ForExpr {
            for_token,
            pat,
            in_token,
            expr,
            body,
        }: ForExpr<E>,
        ctx: Ctx,
        build: &mut Builder,
    ) {
        build.push_tokens(quote!(#for_token #pat #in_token (#expr)));
        self.block(body, ctx, build);
    }

    fn control_flow_while<E: Into<Element>>(
        &self,
        WhileExpr {
            while_token,
            cond,
            body,
        }: WhileExpr<E>,
        ctx: Ctx,
        build: &mut Builder,
    ) {
        build.push_tokens(quote!(#while_token #cond));
        self.block(body, ctx, build);
    }

    fn control_flow_match<E: Into<Element>>(
        &self,
        MatchExpr {
            match_token,
            expr,
            brace_token,
            arms,
        }: MatchExpr<E>,
        ctx: Ctx,
        build: &mut Builder,
    ) {
        let arms = {
            let mut build = self.builder();
            for MatchArm {
                pat,
                guard,
                fat_arrow_token,
                body,
                comma_token,
            } in arms
            {
                build.push_tokens(quote!(#pat));
                if let Some((if_token, cond)) = guard {
                    build.push_tokens(quote!(#if_token #cond));
                }
                build.push_tokens(quote!(#fat_arrow_token));
                self.block(
                    Block {
                        brace_token: Brace(Span::call_site()),
                        markups: Markups {
                            markups: vec![body],
                        },
                    },
                    ctx,
                    &mut build,
                );
                build.push_tokens(quote!(#comma_token));
            }
            build.finish()
        };

        let mut arm_block = TokenStream::new();

        brace_token.surround(&mut arm_block, |tokens| {
            arms.to_tokens(tokens);
        });

        build.push_tokens(quote!(#match_token #expr #arm_block));
    }
}

////////////////////////////////////////////////////////

/// Classifies a `script` element by its `type` attribute.
///
/// Only a single static `type` can mark a script as data. A missing, empty,
/// spliced, optional, toggled or repeated `type` makes it executable, which is
/// the strictest row.
fn script_kind(attrs: &[Attribute]) -> ScriptKind {
    let mut types = named_attrs(attrs, "type");
    match (types.next(), types.next()) {
        (
            Some(AttributeType::Normal {
                value: Markup::Lit(lit),
                ..
            }),
            None,
        ) => ScriptKind::from_type(&lit.to_string()),
        _ => ScriptKind::Executable,
    }
}

/// The values of every attribute called `name`, ignoring ASCII case.
fn named_attrs<'a>(
    attrs: &'a [Attribute],
    name: &'a str,
) -> impl Iterator<Item = &'a AttributeType> + 'a {
    attrs.iter().filter_map(move |attr| match attr {
        Attribute::Named {
            name: attr_name,
            attr_type,
        } if attr_name.to_string().eq_ignore_ascii_case(name) => Some(attr_type),
        _ => None,
    })
}

/// A run of string literals that end up next to each other in the output.
#[derive(Default)]
struct Run {
    text: String,
    /// The byte offset in `text` where each literal starts, and its span.
    literals: Vec<(usize, Span)>,
}

impl Run {
    /// The index of the literal that contains byte `offset` of `text`.
    fn literal_at(&self, offset: usize) -> usize {
        self.literals
            .iter()
            .rposition(|(start, _)| *start <= offset)
            .unwrap_or(0)
    }

    fn span(&self, literal: usize) -> Span {
        self.literals[literal].1
    }
}

/// Splits a body into runs of adjacent literals. A splice or control flow ends
/// a run; the branches of control flow are split separately.
fn static_runs(markups: &[Markup<Element>], current: &mut Run, runs: &mut Vec<Run>) {
    for markup in markups {
        match markup {
            Markup::Lit(lit) => {
                current.literals.push((current.text.len(), lit.span()));
                current.text.push_str(&lit.to_string());
            }
            Markup::Block(block) => static_runs(&block.markups.markups, current, runs),
            Markup::Semi(_)
            | Markup::ControlFlow(ControlFlow {
                kind: ControlFlowKind::Let(_),
                ..
            }) => {}
            Markup::Splice { .. } | Markup::Element(_) => runs.push(std::mem::take(current)),
            Markup::ControlFlow(control_flow) => {
                runs.push(std::mem::take(current));
                let mut branch = |markups: &[Markup<Element>]| {
                    static_runs(markups, current, runs);
                    runs.push(std::mem::take(current));
                };
                match &control_flow.kind {
                    ControlFlowKind::Let(_) => {}
                    ControlFlowKind::If(if_) => {
                        let mut if_ = if_;
                        loop {
                            branch(&if_.then_branch.markups.markups);
                            match if_.else_branch.as_ref().map(|(_, _, b)| &**b) {
                                Some(IfOrBlock::If(next)) => if_ = next,
                                Some(IfOrBlock::Block(block)) => {
                                    branch(&block.markups.markups);
                                    break;
                                }
                                None => break,
                            }
                        }
                    }
                    ControlFlowKind::For(for_) => branch(&for_.body.markups.markups),
                    ControlFlowKind::While(while_) => branch(&while_.body.markups.markups),
                    ControlFlowKind::Match(match_) => {
                        for arm in &match_.arms {
                            branch(std::slice::from_ref(&arm.body));
                        }
                    }
                }
            }
        }
    }
}

/// Whether a body contains any Rust code, so that its output is only known at
/// run time, and it may be left early.
fn has_dynamic_content(markups: &[Markup<Element>]) -> bool {
    markups.iter().any(|markup| match markup {
        Markup::Lit(_) | Markup::Semi(_) | Markup::Element(_) => false,
        Markup::Block(block) => has_dynamic_content(&block.markups.markups),
        // Including `@let`, whose initializer can `break` or `return`.
        Markup::Splice { .. } | Markup::ControlFlow(_) => true,
    })
}

#[allow(clippy::type_complexity)]
fn split_attrs(
    attrs: Vec<Attribute>,
) -> (
    Vec<(HtmlNameOrMarkup, Option<Expr>)>,
    Option<HtmlNameOrMarkup>,
    Vec<(HtmlName, AttributeType)>,
) {
    let mut classes = vec![];
    let mut id = None;
    let mut named_attrs = vec![];

    for attr in attrs {
        match attr {
            Attribute::Class { name, toggler, .. } => {
                classes.push((name, toggler.map(|toggler| toggler.cond)))
            }
            Attribute::Id { name, .. } => id = Some(name),
            Attribute::Named { name, attr_type } => named_attrs.push((name, attr_type)),
        }
    }

    (classes, id, named_attrs)
}

////////////////////////////////////////////////////////

struct Builder {
    output_ident: Ident,
    tokens: TokenStream,
    tail: String,
}

impl Builder {
    fn new(output_ident: Ident) -> Builder {
        Builder {
            output_ident,
            tokens: TokenStream::new(),
            tail: String::new(),
        }
    }

    fn push_str(&mut self, string: &str) {
        self.tail.push_str(string);
    }

    fn push_escaped(&mut self, string: &str) {
        escape::escape_to_string(string, &mut self.tail);
    }

    fn push_tokens(&mut self, tokens: TokenStream) {
        self.cut();
        self.tokens.extend(tokens);
    }

    fn cut(&mut self) {
        if self.tail.is_empty() {
            return;
        }
        let push_str_expr = {
            let output_ident = self.output_ident.clone();
            let tail = &self.tail;
            quote!(#output_ident.push_str(#tail);)
        };
        self.tail.clear();
        self.tokens.extend(push_str_expr);
    }

    fn finish(mut self) -> TokenStream {
        self.cut();
        self.tokens
    }
}
