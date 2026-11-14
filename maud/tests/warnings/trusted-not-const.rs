use maud::trusted_script;

fn main() {
    let code = String::from("alert(1)");
    let _ = trusted_script!(&code);
    let leaked: &'static str = Box::leak(code.into_boxed_str());
    let _ = trusted_script!(leaked);
}
