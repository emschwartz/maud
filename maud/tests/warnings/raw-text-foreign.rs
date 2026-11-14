use maud::{html, trusted_script};

fn main() {
    let user = String::from("alert(document.domain)");
    let code = trusted_script!("a<b>c");
    let para = html! { p {} };
    let mut i = 0;

    html! {
        // F1
        svg { script { (user) } }
        // F2
        svg { @for _ in 0..2 { script { (user) } b {} } }
        // F2b
        svg { @while { i += 1; i <= 2 } { style { (user) } script { (user) } p {} } }
        // F3
        svg { (para) script { (user) } }
        // F4
        select { svg { script { (user) } } }
        select { option { math { script { (user) } } } }
        // F6
        math { svg { foreignObject { p {} } script { (user) } } }
        math { svg { desc { b {} } } style { "x" } script { (user) } }
        math { mi { mglyph { svg { foreignObject { p {} } script { (user) } } } } }
        // F7
        math { mtext { mglyph { script { (code) } } } }
        math { svg { foreignObject { script { (code) } } } }
    };
}
