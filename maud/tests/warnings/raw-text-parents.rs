use maud::html;

fn main() {
    html! {
        textarea { script { "var s = '</textarea><img src=x onerror=alert(1)>';" } }
        title { script { "var s = '</title><img src=x onerror=alert(1)>';" } }
        textarea { style { "a::after { content: '</textarea><img src=x onerror=alert(1)>' }" } }
        xmp { script { "var s = '</xmp><img src=x onerror=alert(1)>';" } }
        iframe { script {} }
        noembed { style {} }
        noframes { script {} }
        plaintext { script {} }
        html { head {} frameset { script { "var s = '<frame onload=alert(1)>';" } } }
        textarea { div { script {} } }
    };
}
