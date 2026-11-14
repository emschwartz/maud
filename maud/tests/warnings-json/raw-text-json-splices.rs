use maud::{html, json::Json};

fn main() {
    let name = String::from("x");

    // These types all run, so JSON is not accepted.
    html! {
        script { (Json(1)) }
        script type="TEXT/JAVASCRIPT" { (Json(1)) }
        script type="module" { (Json(1)) }
        script type="" { (Json(1)) }
        script type=("application/json") { (Json(1)) }
        script type="application/json; charset=utf-8" { (Json(1)) }
        style { (Json(1)) }
    };

    // A JSON block only accepts JSON.
    html! {
        script type="application/json" { (name) }
        script type="application/json" { (maud::trusted_script!("1")) }
    };
}
