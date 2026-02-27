// Copyright © 2024 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

#![doc = include_str!("../README.md")]

mod args;
mod cli;
mod cm;

use std::process::exit;

fn main() {
    if let Err(e) = cm::cm() {
        if let Some(e) = e.downcast_ref::<cm::CommandFailedError>() {
            exit(e.0.unwrap_or(-1));
        } else {
            eprintln!("Error: {e:?}");
            exit(-1);
        }
    }
}
