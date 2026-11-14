use maud::html;

fn main() {
    let user = "alert(document.domain)";

    // The browser splits each of these names, and sees a `type` before the
    // one that `html!` classifies the script by.
    html! {
        script " type "="text/javascript" type="text/template" { (user) }
        script "defer type"="module" type="text/template" { (user) }
        script "type "="module" type="text/template" { (user) }
        script "type=module" type="text/template" { (user) }
        script "type/" type="text/template" { (user) }
        script data-"x type"="" type="text/x-template" { (user) }
        script "x type=importmap y"="" type="text/template" {}
        script "a/type"="module" type="text/template" {}
        script "type\t"="module" type="text/template" {}
        script "type\n"="module" type="text/template" {}
        script "type\r"="module" type="text/template" {}
        script "type\x0c"="module" type="text/template" {}
        script "a>b"="" {}
        script "a\"b"="" {}
        script "a'b"="" {}
        script "a<b"="" {}
        script "a\0b"="" {}
    };

    // Not only on `script`.
    html! {
        div "x onclick"="alert(1)" {}
    };
}
