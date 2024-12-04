// Copyright © 2024 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use crate::cli::{Activate, Build, Cli, Command, Configure, Deactivate, Lit, Quirks};
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use shell_quote::bash::quote;
use std::env;
use std::error;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::io::ErrorKind::NotFound;
use std::path::{Path, PathBuf};
use std::process::{self, Stdio};

type Result<T> = ::std::result::Result<T, Box<dyn error::Error>>;

/// Newtype to capture exit codes from failing commands, as we want to handle these differently
/// than generic failures.
#[derive(Debug)]
pub struct CommandFailedError(pub Option<i32>);
impl error::Error for CommandFailedError {}
impl fmt::Display for CommandFailedError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(i) => write!(f, "command failed with code {}", i),
            None => write!(f, "command failed with unknown code"),
        }
    }
}

#[derive(Deserialize)]
struct ResultDB {
    tests: Vec<ResultDBTest>,
}

impl ResultDB {
    fn parse(paths: Paths) -> Result<ResultDB> {
        let file = File::open(lit_json_path(paths)?)?;
        let reader = BufReader::new(file);
        Ok(serde_json::from_reader(reader)?)
    }
}

#[derive(Deserialize)]
struct ResultDBTest {
    expected: bool,
    #[serde(rename = "testId")]
    test_id: String,
}

impl ResultDBTest {
    fn test_path(&self, paths: Paths) -> PathBuf {
        fn case(find: &'static str, replace: &'static str) -> (Regex, &'static str) {
            // We unwrap because an error compiling the regex is a dev-time failure
            (Regex::new(find).unwrap(), replace)
        }
        lazy_static! {
            static ref REGEXES: Vec<(Regex, &'static str)> = vec![
                case(r"LLVM :: ", "test/"),
                case(r"LLVM-Unit :: .*", "test/Unit"),
                case(r"LLVM :: ", "test/"),
                case(r"LLVM-Unit :: .*", "test/Unit"),
                case(r"Clang :: ", "../clang/test/"),
                case(r"Clang-Unit :: .*", "../clang/test/Unit"),
                case(r"Flang :: ", "../flang/test/"),
                case(r"flang-OldUnit :: .*", "../flang/test/NonGtestUnit"),
                case(r"flang-Unit :: .*", "../flang/test/Unit"),
                case(r"lld :: ", "../lld/test/"),
                case(r"lldb :: ", "../lldb/test/"),
                case(r"lldb-shell :: .*", "../lldb/test/Shell"),
                case(r"lldb-unit :: .*", "../lldb/test/Unit"),
                case(r"lldb-api :: .*", "../lldb/test/API"),
                case(r"MLIR :: ", "../mlir/test/"),
                case(r"MLIR-Unit .*:: ", "../mlir/test/Unit"),
                case(r"libomptarget :: [^:]* :: ", "../openmp/libomptarget/test/"),
                case(r"ompt-test :: ", "../openmp/libompd/test/"),
                case(r"libomp :: ", "../openmp/runtime/test/"),
                case(r"OMPT multiplex :: ", "../openmp/tools/multiplex/tests/"),
                case(r"libarcher :: ", "../openmp/tools/archer/tests/"),
                case(r"Polly :: ", "../polly/test/"),
                case(r"Polly-Unit :: .*", "../polly/test/Unit"),
                case(r"Polly - isl unit tests :: .*", "../polly/test/UnitIsl"),
            ];
        }
        for (find, replace) in REGEXES.iter() {
            if find.is_match(&self.test_id) {
                let mut path = paths.source.to_owned();
                path.push(find.replace(&self.test_id, &**replace).into_owned());
                return path;
            }
        }
        // If we don't recognize the test_id, assume it is a literal path.
        // Should it be something else llvm-lit will complain for us, anyway.
        self.test_id.clone().into()
    }
}

#[derive(Clone, Copy)]
struct Paths<'a> {
    source: &'a Path,
    binary: &'a Path,
}

fn plan_configure(
    configure: &Configure,
    cli: &Cli,
    quirks: Quirks,
    paths: Paths,
) -> Result<Vec<process::Command>> {
    let mut cmd = process::Command::new("cmake");
    let mut flags = Vec::new();
    cmd.arg("-S");
    cmd.arg(paths.source.as_os_str());
    cmd.arg("-B");
    cmd.arg(paths.binary.as_os_str());
    if configure.makefiles {
        cmd.args(["-G", "Unix Makefiles"]);
    } else {
        cmd.args(["-G", &*configure.generator]);
    }
    cmd.arg(format!("-DCMAKE_BUILD_TYPE={}", cli.final_config()));
    cmd.arg(format!(
        "-DCMAKE_PREFIX_PATH={}",
        configure.prefix_path.join(";")
    ));
    cmd.arg("-DCMAKE_INSTALL_PREFIX=dist");
    cmd.arg("-DCMAKE_EXPORT_COMPILE_COMMANDS=On");
    if let Quirks::Llvm = quirks {
        cmd.arg("-DLLVM_ENABLE_ASSERTIONS=On");
        cmd.arg("-DLLVM_OPTIMIZED_TABLEGEN=On");
        if has_command("sphinx-build")? {
            cmd.arg("-DLLVM_ENABLE_SPHINX=On");
        }
        if has_command("lld")? && has_cc_flag("-fuse-ld=lld")? {
            cmd.arg("-DLLVM_USE_LINKER=lld");
        } else if has_command("gold")? && has_cc_flag("-fuse-ld=gold")? {
            cmd.arg("-DLLVM_USE_LINKER=gold");
        }
    }
    if has_command("ccache")? {
        match quirks {
            Quirks::None => {
                cmd.arg("-DCMAKE_C_COMPILER_LAUNCHER=ccache");
                cmd.arg("-DCMAKE_CXX_COMPILER_LAUNCHER=ccache");
            }
            Quirks::Llvm => {
                cmd.arg("-DLLVM_CCACHE_BUILD=On");
            }
        }
    }
    if has_cc_flag("-fcolor-diagnostics")? {
        flags.push("-fcolor-diagnostics".into());
    }
    if configure.san {
        match quirks {
            Quirks::None => {
                flags.push("-fsanitize=address,undefined".into());
            }
            Quirks::Llvm => {
                cmd.arg("-DLLVM_USE_SANITIZER=Address;Undefined");
                cmd.arg("-DLLVM_USE_SANITIZE_COVERAGE=Yes");
            }
        }
    }
    if configure.expensive_checks {
        cmd.arg("-DLLVM_ENABLE_EXPENSIVE_CHECKS=On");
        cmd.arg("-DLLVM_ENABLE_WERROR=Off");
    }
    cmd.arg(format!(
        "-DLLVM_ENABLE_PROJECTS={}",
        configure
            .enable_projects
            .as_ref()
            .map_or("llvm;clang;lld".into(), |v| v.join(";"))
    ));
    cmd.arg(format!(
        "-DLLVM_TARGETS_TO_BUILD={}",
        configure
            .targets_to_build
            .as_ref()
            .map_or("all".into(), |v| v.join(";"))
    ));
    let flags = flags
        .iter()
        .chain(configure.flag.iter())
        .map(|s| &**s)
        .collect::<Vec<_>>()
        .join(" ");
    let maybe_prepend_space = |mut s: String| {
        if !flags.is_empty() {
            s.insert(0, ' ')
        }
        s
    };
    let env_cflags = env::var("CFLAGS")
        .map(maybe_prepend_space)
        .unwrap_or_default();
    let env_cxxflags = env::var("CXXFLAGS")
        .map(maybe_prepend_space)
        .unwrap_or_default();
    cmd.arg(format!("-DCMAKE_C_FLAGS={}{}", flags, env_cflags));
    cmd.arg(format!("-DCMAKE_CXX_FLAGS={}{}", flags, env_cxxflags));
    cmd.args(configure.args.as_slice());
    let mut rm_cmd = process::Command::new("rm");
    rm_cmd.arg("-rf");
    let mut cache_path = paths.binary.to_owned();
    cache_path.push("CMakeCache.txt");
    rm_cmd.arg(cache_path);
    let mut files_path = paths.binary.to_owned();
    files_path.push("CMakeFiles");
    rm_cmd.arg(files_path);
    Ok(vec![rm_cmd, cmd])
}

fn build_cmd(cli: &Cli, paths: Paths) -> process::Command {
    let mut cmd = process::Command::new("cmake");
    cmd.arg("--build");
    cmd.arg(paths.binary);
    cmd.arg("--config");
    cmd.arg(cli.final_config());
    cmd.arg("--");
    cmd
}

fn plan_build(
    build: &Build,
    cli: &Cli,
    _quirks: Quirks,
    paths: Paths,
) -> Result<Vec<process::Command>> {
    let mut cmd = build_cmd(cli, paths);
    cmd.args(build.args.as_slice());
    Ok(vec![cmd])
}

fn plan_lit(lit: &Lit, cli: &Cli, _quirks: Quirks, paths: Paths) -> Result<Vec<process::Command>> {
    if lit.xfail_export {
        let mut cmd = process::Command::new("printf");
        cmd.arg("%s\\n");
        cmd.arg(format!(
            "export LIT_XFAIL=\"{}\"",
            ResultDB::parse(paths)?
                .tests
                .iter()
                .filter(|t| !t.expected)
                .map(|t| &*t.test_id)
                .collect::<Vec<_>>()
                .join(";")
        ));
        return Ok(vec![cmd]);
    }
    if let Some(group) = &lit.group {
        let mut cmd = build_cmd(cli, paths);
        cmd.arg(group);
        if lit.update_resultdb {
            add_lit_opts_env(&mut cmd, paths)?;
        }
        return Ok(vec![cmd]);
    }
    let mut args: Vec<PathBuf> = if lit.tests.is_empty() {
        match ResultDB::parse(paths) {
            Ok(rdb) => rdb
                .tests
                .into_iter()
                .filter(|t| !t.expected)
                .take(if lit.first { 1 } else { usize::MAX })
                .map(|t| t.test_path(paths))
                .collect(),
            Err(e) => {
                eprintln!("warning: ignoring lit.json: {}", e);
                vec![]
            }
        }
    } else {
        lit.tests.iter().map(|a| a.into()).collect()
    };
    args.extend(lit.args.iter().map(|a| a.into()));
    if args.is_empty() {
        Ok(vec![])
    } else if lit.print_only {
        let mut cmd = process::Command::new("printf");
        cmd.arg("%s\\n");
        cmd.args(args);
        Ok(vec![cmd])
    } else {
        let mut lit_path = paths.binary.to_path_buf();
        lit_path.push("bin/llvm-lit");
        let mut cmd = process::Command::new(lit_path);
        if lit.verbose {
            cmd.env("FILECHECK_OPTS", "--dump-input always");
            cmd.arg("-a");
        }
        cmd.args(args);
        if lit.update_resultdb {
            add_lit_opts_env(&mut cmd, paths)?;
        }
        Ok(vec![cmd])
    }
}

fn plan_activate(
    _activate: &Activate,
    cli: &Cli,
    _quirks: Quirks,
    paths: Paths,
) -> Result<Vec<process::Command>> {
    let mut cmd = process::Command::new("printf");
    cmd.arg(
        "CM_SRC=%s CM_BIN=%s CM_CFG=%s;\\n\
        export CM_SRC CM_BIN CM_CFG;\\n\
        PATH=\"$CM_BIN/bin:$PATH\";\\n\
        alias cm='cm -s \"$CM_SRC\" -b \"$CM_BIN\" -c \"$CM_CFG\"';\\n",
    );
    cmd.arg(quote(paths.source));
    cmd.arg(quote(paths.binary));
    cmd.arg(quote(cli.final_config()));
    Ok(vec![cmd])
}

fn plan_deactivate(
    _deactivate: &Deactivate,
    _cli: &Cli,
    _quirks: Quirks,
    _paths: Paths,
) -> Result<Vec<process::Command>> {
    let mut cmd = process::Command::new("printf");
    cmd.arg(
        "unalias cm;\\n\
        [ -z \"$CM_BIN\" ] || PATH=\"${PATH/$CM_BIN\\/bin:/}\";\\n\
        unset -v CM_SRC CM_BIN CM_CFG;\\n",
    );
    Ok(vec![cmd])
}

fn plan(
    command: &Command,
    cli: &Cli,
    quirks: Quirks,
    paths: Paths,
) -> Result<Vec<process::Command>> {
    match command {
        Command::Configure(ref c) => plan_configure(c, cli, quirks, paths),
        Command::Build(ref b) => plan_build(b, cli, quirks, paths),
        Command::Lit(ref l) => plan_lit(l, cli, quirks, paths),
        Command::Activate(ref a) => plan_activate(a, cli, quirks, paths),
        Command::Deactivate(ref d) => plan_deactivate(d, cli, quirks, paths),
    }
}

fn lit_json_path(paths: Paths) -> Result<PathBuf> {
    let mut path = paths.binary.canonicalize()?;
    path.push("lit.json");
    Ok(path)
}

fn add_lit_opts_env(cmd: &mut process::Command, paths: Paths) -> Result<()> {
    let mut lit_opts = OsString::from("--resultdb-output ");
    lit_opts.push(quote(lit_json_path(paths)?.as_os_str()));
    cmd.env("LIT_OPTS", lit_opts);
    Ok(())
}

fn has_command(name: &str) -> Result<bool> {
    if env::var("CM_TESTING").is_ok() {
        return Ok(true);
    }
    let status = process::Command::new(name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn has_cc_flag(name: &str) -> Result<bool> {
    let cc = env::var("CC").unwrap_or("cc".into());
    let status = process::Command::new(cc)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args(["-x", "c", "-", "-o", "-", "-c"])
        .arg(name)
        .status();
    match status {
        Ok(o) => Ok(o.success()),
        Err(e) if e.kind() == NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn detect_quirks(cli: &Cli) -> Quirks {
    let source = cli.source.clone().unwrap_or(".".into());
    let mut cml = source.clone();
    cml.push(r"CMakeLists.txt");
    let mut llvm = source.clone();
    llvm.push(r"llvm");
    if !cml.is_file() && llvm.is_dir() {
        Quirks::Llvm
    } else {
        Quirks::None
    }
}

pub fn cm(cli: Cli) -> Result<()> {
    let quirks = cli.quirks.unwrap_or(detect_quirks(&cli));
    let source = cli.source.clone().unwrap_or(match quirks {
        Quirks::None => ".".into(),
        Quirks::Llvm => "llvm".into(),
    });
    let binary = cli.binary.clone().unwrap_or("build".into());
    let paths = Paths {
        source: &source,
        binary: &binary,
    };
    let cmds = plan(&cli.command, &cli, quirks, paths)?;
    for ref mut cmd in cmds {
        if cli.dry_run {
            let mut quoted = Vec::new();
            quoted.extend(cmd.get_envs().filter_map(|(key, val)| {
                Some(format!(
                    "{}={}",
                    quote(key).to_string_lossy(),
                    quote(val?).to_string_lossy(),
                ))
            }));
            quoted.push(quote(cmd.get_program()).to_string_lossy().into_owned());
            quoted.extend(
                cmd.get_args()
                    .map(|arg| quote(arg).to_string_lossy().into_owned()),
            );
            println!("{}", quoted.join(" "));
        } else {
            let status = cmd.status()?;
            if !status.success() {
                return Err(Box::new(CommandFailedError(status.code())));
            };
        }
    }
    Ok(())
}
