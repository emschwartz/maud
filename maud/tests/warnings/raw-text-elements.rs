use maud::html;

fn main() {
    html! {
        script { b { "x" } }
        style { @if true { p {} } }
        script type="text/template" { div {} }
        script src="app.js";
        style;
    };
}
