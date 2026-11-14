use maud::{html, PreEscaped};

fn main() {
    let name = String::from("x");
    let markup = html! { "x" };

    html! {
        script { "var a = '" (name) "';" }
        script { (name.as_str()) }
        script { (markup) }
        script { (PreEscaped("alert(1)")) }
        script { (1) }
        style { (name) }
        style { (PreEscaped("a {}")) }
        style { (maud::trusted_script!("1")) }
    };
}
