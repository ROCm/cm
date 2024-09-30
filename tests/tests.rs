#[test]
fn cmd_tests() {
    trycmd::TestCases::new()
        .env("CC", "/bin/false")
        .env("CFLAGS", "--user-c-flag")
        .env("CXXFLAGS", "--user-cxx-flag")
        .case("tests/cmd/*.toml");
}
