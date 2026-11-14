use maud::html;

fn main() {
    html! {
        script { "</script><img src=x onerror=alert(1)>" }
        script { "a</ScRiPt>" }
        script { "<!--<script>" }
        script { "<" "/script>" }
        script { "</scr" { "ipt>" } }
        script type="application/ld+json" { "{}</script>" }
        style { "</STYLE><script>" }
        style { "</sty" "le>" }
    };

    html! {
        script {
            @if true { "</script>" }
        }
    };
}
