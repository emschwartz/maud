use maud::{html, json::Json};

fn main() {
    let user = [("imports", [("app", "https://evil.example/x.js")])];

    html! {
        script "type "="importmap" type="application/json" { (Json(&user)) }
        script "x type=importmap y"="" type="application/json" { (Json(&user)) }
        script "a/type"="module" type="application/json" { (Json(&user)) }
        script "type=text/javascript" type="application/json" {
            "window.cfg = " (Json(&user)) ";"
        }
    };
}
