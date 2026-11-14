use maud::{trusted_script, trusted_style};

fn main() {
    let _ = trusted_script!("</script>");
    let _ = trusted_script!(concat!("<!--<scr", "ipt>"));
    let _ = trusted_style!("</style>");
    if false {
        // Not reached at run time, but still checked at compile time.
        let _ = trusted_script!("</SCRIPT>");
    }
}
