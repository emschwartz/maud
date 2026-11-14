use maud::{TrustedScript, TrustedStyle};

fn main() {
    let user = String::from("alert(document.domain)");
    // The constructor behind the macros is not public.
    let _ = TrustedScript::from_const_internal(String::leak(user.clone()));
    let _ = TrustedStyle::from_const_internal(String::leak(user.clone()));
    let _ = TrustedScript::from_static(String::leak(user));
}
