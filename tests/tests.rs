// Copyright © 2024 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

#[test]
fn cmd_tests() {
    trycmd::TestCases::new()
        .env("CM_TESTING", "")
        .env("CC", "/bin/false")
        .env("CFLAGS", "--user-c-flag")
        .env("CXXFLAGS", "--user-cxx-flag")
        .case("tests/cmd/*.toml");
}
