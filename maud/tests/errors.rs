use trybuild::TestCases;

#[test]
fn run_warnings() {
    let config = TestCases::new();
    config.compile_fail("tests/warnings/*.rs");
    #[cfg(feature = "json")]
    config.compile_fail("tests/warnings-json/*.rs");
}
