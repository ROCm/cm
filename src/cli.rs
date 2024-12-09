// Copyright © 2024 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use clap::{
    builder::{ArgAction, ArgPredicate, PossibleValue, TypedValueParser},
    error::{ContextKind, ContextValue},
    ArgGroup, Args, Parser, Subcommand, ValueHint,
};
use std::ffi::OsString;
use std::path::PathBuf;

type ClapError = clap::Error;
type ClapErrorKind = clap::error::ErrorKind;

const DIR_HEADING: Option<&str> = Some("CMAKE DIRECTORY OPTIONS");
const LLVM_HEADING: Option<&str> = Some("LLVM-SPECIFIC OPTIONS");

/// Frontend for configuring/building/testing CMake projects (see --help for more details)
///
/// Provides a common interface with saner defaults for working with CMake projects, including
/// special support for smoothing over quirks when compiling LLVM.
///
/// The default cmake command-line presents multiple inconsistent interfaces for related tasks,
/// namely:
///
/// * The configure step generally requires -S for specifying the "source" path and -B for
///   specifying the "build" path (also referred to as the "binary" path in some places).
///
/// * The build step either requires directly invoking the build tool, with its own unique syntax
///   for specifying the build path again, or:
///
/// * The built-in, generic interface to the build tool in cmake requires a _different_ syntax for
///   specifying the build path, no longer accepting -B at all.
///
/// Other tasks, like specifying the type of build (Release, Debug, RelWithDbgInfo, ...), are also
/// needlessly fragmented between configuration (-DCMAKE_BUILD_TYPE=...) and build (--config ...).
///
/// These tasks are also not on equal footing, with configure being the "default"
/// and build being a pseudo-subcommand "--build".
///
/// This tool exposes the "configuration" and "build" tasks as subcommands directly, and give them
/// a common interface for specifying the source (-s, --source) and binary (-b, --binary) paths, as
/// well as the config (-c, --config).
///
/// Beyond the tools themselves, individual projects also use cmake in quirky ways. In particular,
/// LLVM makes some strange choices:
///
/// * The root "CMakeLists.txt" is in the "llvm/" directory, not at the root of the project.
///
/// * Does not respect CMAKE_{C,CXX}_COMPILER_LAUNCHER, instead invents LLVM_CCACHE_BUILD.
///
/// It also has many knobs whose defaults are not appropriate for development, such as disabling
/// assertions by default, and it has many targets and optional projects which can be
/// enabled/disabled with particular cache variables.
///
/// To make LLVM behave like "the ideal project" as much as possible, this tool expects the source
/// to still be specified as the root llvm-project repository. By default, the tool will detect
/// that LLVM is being compiled and update the true source directory accordingly, as well as adjust
/// many default options. There are also flags for LLVM-specific concepts like TARGETS_TO_BUILD and
/// EXPENSIVE_CHECKS to simplify common configuration tweaks.
///
/// Another subcommand "lit" provides a nicer interface to llvm-lit (and cmake --build, to
/// implement the -g/--group flag). The "lit" subcommand optionally ensures that a ResultDB file
/// called "lit.json" is written to the binary path when tests are run, allowing subsequent runs to
/// recall which tests failed. With no arguments or flags specifying which tests to run, the
/// subcommand will run all tests marked as failed in the ResultDB. Repeatedly invoking the
/// subcommand can thus incrementally "resolve" tests as they are updated, removing them from the
/// list of failing tests until it is empty. This behavior is controlled by the
/// -u/--update-resulbdb[=<BOOL>] flag which is enabled by default unless a particular subset of
/// tests is specified (via the -1/--first flag or TESTS arguments). The developer can then focus
/// on specific failing tests without losing track of the remaining failing tests, and can record
/// newly passing tests by running the subcommand without specifying a subset.
///
/// The "lit" subcommand will also manage the FILECHECK_OPTS environment variable to make truely
/// "verbose" lit output easier to achieve.
///
/// Finally, the subcommands "activate" and "deactivate" print shell commands to modify the shell
/// environment to "enter" and "exit" a set of global "cm" options. The "activate" command sets
/// variables for the source directory ("CM_SRC"), binary directory ("CM_BIN"), and the
/// configuration ("CM_CFG"), as well as an alias for "cm" which uses them. To simplify executing
/// binaries in the binary directory it also prepends the "bin" subdirectory in the binary path to
/// the "PATH" environment variable. The "deactivate" command attempts to "undo" all of the effects
/// of "activate". The output of each subcommand is intended to be passed as arguments to "eval".
/// Neither subcommand handles all edge cases, nor do they support a wide gamut of shells (yet).
/// One notable case they don't handle gracefully is an empty PATH.
///
/// Typical usage of the tool involves leaving a shell parked at the top-level of the llvm-project
/// and running subcommands (note that the subcommand can be abbreviated):
///
///     $ cm configure      # default values for --source, --binary, and --config are used
///     $ cm build          # ditto
///     $ cm l -g llvm      # Run a test group
///     $ cm l -v
///     $ ...               # Resolve tests failures, referencing full verbose test output
///     $ cm l
///     $ ...
///     $ cm l -1v          # Focus on only one test, implicitly not touching the ResultDB
///     $ ...               # Fix the test
///     $ cm l              # Record the fix into the ResultDB
///
/// With non-default values for --source/--binary/--config you can leave these options alone across
/// subcommands:
///
///     $ cm -s src -b bin -c Debug c
///     $ cm -s src -b bin -c Debug b
///     $ cm -s src -b bin -c Debug b check-llvm
///     $ cm -s src -b bin -c Debug l
///     $ cm -s src -b bin -c Debug l -v
///     $ # ...
///     $ cm -s src -b bin -c Debug l
///
/// With most shells, these values can be factored out with aliases:
///
///     $ alias cm='cm -s src -b bin -c Debug'
///     $ cm configure
///     $ cm build
///     $ cm l -g llvm
///     $ cm l
///     $ cm l -v
///     $ # ...
///     $ cm l
///
/// For the bash shell the "activate" subcommand automates this aliasing, updates "PATH" to search
/// the bin subdirectory in the binary path, and also defines the environment variables "CM_SRC",
/// "CM_BIN", and "CM_CFG" for use in scripts:
///
///     $ type cm
///     cm is /usr/bin/cm
///     $ eval $(cm -s src -b bin -d activate)
///     $ echo "$CM_SRC"
///     src
///     $ echo "$CM_BIN"
///     bin
///     $ echo "$CM_CFG"
///     Debug
///     $ echo "$PATH"
///     bin/bin:$ORIG_PATH
///     $ type cm
///     cm is aliased to `cm -s "$CM_SRC" -b "$CM_BIN" -c "$CM_CFG"'
///
/// And the "deactivate" subcommand automates reversing an "activate":
///
///     $ # beginning with the environment from above...
///     $ eval $(cm deactivate)
///     $ echo "$CM_SRC"
///     $ echo "$CM_BIN"
///     $ echo "$CM_CFG"
///     $ echo "$PATH"
///     $ORIG_PATH
///     $ type cm
///     cm is /usr/bin/cm
///
#[derive(Parser)]
#[command(version, verbatim_doc_comment, infer_subcommands = true)]
#[command(group = ArgGroup::new("conf").multiple(false))]
#[command(group = ArgGroup::new("gen").multiple(false))]
pub struct Cli {
    /// CMake Source Directory
    #[arg(short, long, value_hint = ValueHint::DirPath, global = true, help_heading = DIR_HEADING)]
    pub source: Option<PathBuf>,
    /// CMake Binary Directory
    #[arg(short, long, value_hint = ValueHint::DirPath, global = true, help_heading = DIR_HEADING)]
    pub binary: Option<PathBuf>,
    /// Build Config
    #[arg(short, long, default_value = "Release", group = "conf", global = true)]
    pub config: String,
    /// Shorthand for `--config Debug`
    #[arg(short, long, group = "conf", global = true)]
    pub debug: bool,
    /// Perform a dry run, only printing the generated command line
    #[arg(short = '#', long, global = true)]
    pub dry_run: bool,
    /// Disable quirk mode detection and specify one explicitly
    #[arg(short, long, global = true)]
    pub quirks: Option<Quirks>,
    /// The subcommand
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn final_config(&self) -> String {
        if self.debug {
            "Debug".into()
        } else {
            self.config.clone()
        }
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// CMake Configure
    #[command(visible_alias = "c")]
    Configure(Configure),
    /// CMake Build
    #[command(visible_alias = "b")]
    Build(Build),
    /// llvm-lit
    #[command(visible_alias = "l")]
    Lit(Lit),
    /// Print shell commands to activate a set of global options
    ///
    /// Prepends the PATH environment variable with the bin subdirectory of the binary path, sets
    /// CM_SRC/CM_BIN/CM_CFG, and defines an alias for cm which uses them.
    #[command(visible_alias = "a")]
    Activate(Activate),
    /// Print shell commands to deactivate global options set via activate
    ///
    /// Attempts to remove elements from the PATH environment variable which correspond to the
    /// active CM_BIN, unsets CM_SRC/CM_BIN/CM_CFG, and unaliases cm.
    #[command(visible_alias = "d")]
    Deactivate(Deactivate),
}

#[derive(Args)]
#[command(group = ArgGroup::new("targets").multiple(false))]
pub struct Configure {
    /// Append to CMAKE_PREFIX_PATH [default: empty]
    #[arg(short, long)]
    pub prefix_path: Vec<String>,
    /// Generator
    #[arg(short, long, default_value = "Ninja", group = "gen")]
    pub generator: String,
    /// Shorthand for `-g "Unix Makefiles"`
    #[arg(short, long, group = "gen")]
    pub makefiles: bool,
    /// Append to C_FLAGS and CXX_FLAGS
    #[arg(short, long)]
    pub flag: Vec<String>,
    /// Enable ASan and UBSan
    #[arg(long)]
    pub san: bool,
    /// Enable expensive checks
    #[arg(long, help_heading = LLVM_HEADING)]
    pub expensive_checks: bool,
    /// Append to LLVM_ENABLE_PROJECTS [default: llvm;clang;lld]
    ///
    /// When no project is specified, the default set is used. If any project is specified the
    /// default set is ignored and all specified projects are enabled.
    #[arg(short, long, help_heading = LLVM_HEADING)]
    pub enable_projects: Option<Vec<String>>,
    /// Append to LLVM_TARGETS_TO_BUILD [default: all]
    ///
    /// When no target is specified, the default set is used. If any target is specified, the
    /// default set is ignored and all specified targets _as well as the "Native" target_ are
    /// enabled.
    ///
    /// For example, on an x86_64 host machine, the following command-line will enable X86 and
    /// AMDGPU:
    ///
    ///     $ cm configure -t AMDGPU
    ///
    /// To disable the implicit inclusion of the "Native" target, use the
    /// -T/--targets-to-build-alt flag instead.
    #[arg(short, long, group = "targets", help_heading = LLVM_HEADING)]
    pub targets_to_build: Option<Vec<String>>,
    /// Append to LLVM_TARGETS_TO_BUILD wihout implicit "Native" target [default: all]
    ///
    /// See -t/--targets-to-build help for more details
    #[arg(short = 'T', long, group = "targets", help_heading = LLVM_HEADING)]
    pub targets_to_build_alt: Option<Vec<String>>,
    /// Trailing arguments to forward to cmake
    pub args: Vec<OsString>,
}

#[derive(Args)]
pub struct Build {
    /// Trailing arguments to forward to build tool
    pub args: Vec<OsString>,
}

#[derive(Args)]
#[command(group = ArgGroup::new("select").multiple(false))]
pub struct Lit {
    /// Print tests that would be run
    #[arg(short, long)]
    pub print_only: bool,
    /// Print a command-line which exports LIT_XFAIL to the tests that would be run
    #[arg(short, long)]
    pub xfail_export: bool,
    /// Update the ResultDB file.
    ///
    /// Defaults to true unless -1/--first or a list of tests (via positional arguments) are
    /// specified.
    ///
    /// Accepts explicit argument via -u/--update-resultdb=true or -u/--update-resultdb=false
    /// and has a shorthand -u/--update-resultdb for the former.
    #[arg(short, long, action = ArgAction::Set, value_name = "BOOL", num_args = 0..=1, require_equals = true,
          default_value_t = true,
          default_missing_value = "true",
          default_value_if("first", ArgPredicate::IsPresent, Some("false")),
          default_value_if("tests", ArgPredicate::IsPresent, Some("false")),
    )]
    pub update_resultdb: bool,
    /// Run the named LLVM "check-*" test group, and (by default) update the ResultDB.
    ///
    /// For known groups ("possible values") the name can be shortened by omitting the "check-"
    /// prefix, and only needs to specify enough characters to unambiguously identify the test
    /// group. For example simply "a" is enough to identify "check-all".
    #[arg(short, long, value_parser = LitGroupParser {}, group = "select")]
    pub group: Option<String>,
    /// Only consider at most the first failing test in the ResultDB.
    #[arg(short = '1', long, group = "select")]
    pub first: bool,
    /// Be as verbose as possible, asking FileCheck to dump its input and asking llvm-lit to
    /// forward it to stdout
    #[arg(short, long)]
    pub verbose: bool,
    /// Lit test paths to run
    #[arg(group = "select")]
    pub tests: Vec<OsString>,
    /// Trailing arguments to forward to llvm-lit
    ///
    /// Note that the -- separator is mandatory to signal the beginning of these verbatim
    /// arguments, which is inconsistent with other subcommands like configure and build. This is a
    /// compromise to make explicit passing of tests more ergonomic such that the default case
    /// requires no additional flags or separators.
    #[arg(last = true)]
    pub args: Vec<OsString>,
}

#[derive(Args)]
pub struct Activate {}

#[derive(Args)]
pub struct Deactivate {}

#[derive(clap::ValueEnum, Clone, Copy)]
pub enum Quirks {
    None,
    Llvm,
}

#[derive(Clone, Copy)]
pub struct LitGroupParser {}

impl LitGroupParser {
    const KNOWN_GROUPS: [&'static str; 4] = ["all", "llvm", "clang", "lld"];

    fn error(cmd: &clap::Command, arg: Option<&clap::Arg>, val: impl Into<String>) -> ClapError {
        let mut err = ClapError::new(ClapErrorKind::InvalidValue).with_cmd(cmd);
        if let Some(arg) = arg {
            err.insert(
                ContextKind::InvalidArg,
                ContextValue::String(arg.to_string()),
            );
        }
        err.insert(ContextKind::InvalidValue, ContextValue::String(val.into()));
        // This hint about "check-*" always being legal is only present in the diagnostics so that
        // autocomplete scripts we generate don't get confused by it. Ideally we would also report
        // this in the "possible values" printed as part of --help but we work around this by
        // explicitly documenting it in the relevant help text.
        let mut valid_values = vec!["check-*".to_string()];
        valid_values.extend(Self::KNOWN_GROUPS.into_iter().map(String::from));
        err.insert(ContextKind::ValidValue, ContextValue::Strings(valid_values));
        err
    }
}

impl TypedValueParser for LitGroupParser {
    type Value = String;

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue>>> {
        Some(Box::new(Self::KNOWN_GROUPS.iter().map(PossibleValue::new)))
    }

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, ClapError> {
        let value = value
            .to_str()
            .ok_or_else(|| ClapError::new(ClapErrorKind::InvalidUtf8))?;
        if value.starts_with("check-") {
            return Ok(value.to_string());
        }
        let matching_groups = Self::KNOWN_GROUPS
            .into_iter()
            .filter(|s| s.starts_with(value))
            .collect::<Vec<_>>();
        match matching_groups[..] {
            [unique_group] => Ok(format!("check-{}", unique_group)),
            _ => Err(Self::error(cmd, arg, value)),
        }
    }
}
