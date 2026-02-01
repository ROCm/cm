// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use crate::cli::Globals;
use applause::ArgsToVec;
use clap::{Parser, Subcommand};
use std::env;
use std::error;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{BufRead, BufReader, Lines};
use std::path::PathBuf;

type Result<T> = ::std::result::Result<T, Box<dyn error::Error>>;

#[derive(Default, Debug)]
struct Config {
    inner: Option<ConfigInner>,
}

impl Config {
    fn from_path<P: Into<PathBuf>>(p: P) -> Result<Config> {
        Ok(Config {
            inner: Some(ConfigInner {
                lines: BufReader::new(File::open(p.into())?).lines(),
                section: "".into(),
            }),
        })
    }

    fn from_env() -> Result<Config> {
        Ok(match env::var_os("CM_CONFIG_PATH") {
            None => {
                if env::var("CM_TESTING").is_ok() {
                    return Ok(Default::default());
                }
                match dirs::config_dir() {
                    None => Default::default(),
                    Some(mut p) => {
                        p.push("cm.rc");
                        Self::from_path(p).ok().unwrap_or(Default::default())
                    }
                }
            }
            Some(p) if p.is_empty() => Default::default(),
            Some(p) => Self::from_path(p)?,
        })
    }

    fn slurp_into(mut self, subcommand_prefix: &OsStr, out: &mut Vec<OsString>) -> Result<()> {
        let inner = match &mut self.inner {
            Some(ref mut i) => i,
            _ => return Ok(()),
        };
        while let Some(line) = inner.lines.next() {
            let line = line?;
            if line.starts_with('-') {
                if inner.in_section(subcommand_prefix) {
                    out.push(line.into());
                }
            } else if line.trim_start().starts_with('#') || line.trim().is_empty() {
                continue;
            } else {
                inner.section = line;
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ConfigInner {
    lines: Lines<BufReader<File>>,
    section: String,
}

impl ConfigInner {
    fn in_section(&self, subcommand_prefix: &OsStr) -> bool {
        self.section.is_empty()
            || self
                .section
                .starts_with(subcommand_prefix.to_str().unwrap())
    }
}

/// A reconstructed `cli::Cli` used to "preprocess" the command-line in order
/// to extract the subcommand and its arguments from Clap.
#[derive(Parser)]
#[command(disable_help_flag = true)]
struct PreCli {
    #[clap(flatten)]
    globals: Globals,
    #[clap(short = 'h')]
    help_short: bool,
    #[clap(long = "help")]
    help_long: bool,
    #[command(subcommand)]
    command: PreCliSub,
}

#[derive(Subcommand)]
enum PreCliSub {
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

/// Get the "cooked" args vector, incorporating the config file (if any) and moving everything
/// under the subcommand.
pub fn build() -> Result<Vec<OsString>> {
    if let Ok(pre_cli) = PreCli::try_parse() {
        build_with_pre_cli(pre_cli)
    } else {
        Ok(env::args_os().collect())
    }
}

fn build_with_pre_cli(pre_cli: PreCli) -> Result<Vec<OsString>> {
    let mut args = vec![];
    let PreCliSub::External(mut sub_and_args) = pre_cli.command;
    let mut sub_args = sub_and_args.split_off(1);
    let sub = sub_and_args.into_iter().next().unwrap();
    if let Some(bin) = env::args_os().next() {
        args.push(bin);
    }
    args.push(sub.clone());
    Config::from_env()?.slurp_into(sub.as_os_str(), &mut args)?;
    args.extend(pre_cli.globals.args_to_vec());
    if pre_cli.help_short {
        args.push("-h".into());
    }
    if pre_cli.help_long {
        args.push("--help".into());
    }
    args.append(&mut sub_args);
    Ok(args)
}
