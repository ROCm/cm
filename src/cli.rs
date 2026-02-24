// Copyright © 2024 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use applause::{Bool, FuzzyParser, OverridingVec, SettableBool};
use applause_derive::ArgsToVec;
use clap::{
    builder::{ArgAction, ArgPredicate},
    ArgGroup, Args, Parser, Subcommand, ValueHint,
};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

const GLOBAL_HEADING: Option<&str> = Some("Global Options");
const LLVM_HEADING: Option<&str> = Some("LLVM-Specific Options");

/// Frontend for configuring/building/testing CMake projects (see --help for more details)
///
/// Provides a subcommand-based interface with saner defaults for working with CMake projects,
/// with special support for LLVM.
///
/// All subcommands share a common interface for specifying the source (-s/--source) and binary
/// (-b/--binary) paths, as well as the config (-c/--config).
///
/// Typical usage of the tool involves leaving a shell parked at the top-level of the CMake project
/// and running subcommands (note that the subcommand can be abbreviated):
///
///     $ cm configure      # default values for --source, --binary, and --config are used
///     $ cm build          # ditto
///     $ # assuming the project is LLVM...
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
///     $ cm -s src -b bin -c debug c
///     $ cm -s src -b bin -c debug b
///     $ cm -s src -b bin -c debug b check-llvm
///     $ cm -s src -b bin -c debug l
///     $ cm -s src -b bin -c debug l -v
///     $ # ...
///     $ cm -s src -b bin -c debug l
///
/// For the bash and zsh shells the "activate" subcommand automates pinning these values via
/// environment variables and updates "PATH" to search the bin subdirectory in the binary path:
///
///     $ eval $(cm -s src -b bin -c debug activate)
///     $ echo "$CM_SRC"
///     $PWD/src
///     $ echo "$CM_BIN"
///     $PWD/bin
///     $ echo "$CM_CFG"
///     Debug
///     $ echo "$PATH"
///     $PWD/bin/bin:$ORIG_PATH
///
/// Tip: Including these variable in your shell's prompt can make it easier to track when you have
/// activated/deactivated for different projects.
///
/// Tip: The source and binary paths are converted to absolute paths, so once activated you can
/// also change directories and continue to run cm commands and binaries from your build without
/// having to locate the project root.
///
/// The "deactivate" subcommand automates reversing an "activate":
///
///     $ # beginning with the environment from above...
///     $ eval $(cm deactivate)
///     $ echo "$CM_SRC"
///     $ echo "$CM_BIN"
///     $ echo "$CM_CFG"
///     $ echo "$PATH"
///     $ORIG_PATH
///
/// Global configuration is read from a file named "cm.rc" in the platform-specific user config
/// directory (e.g. on Linux this is probably $XDG_CONFIG_HOME/cm.rc or ~/.config/cm.rc). This can
/// be controlled by setting the environment variable CM_CONFIG_PATH: if set to the empty string
/// then no configuration file is used, and otherwise the value is interpreted as an alternative
/// path to a config file to read.
///
/// The config file format is line-based, where each line is either:
///
/// * A comment, starting with '#'
/// * An argument, starting with '-' and being interpreted verbatim (i.e. no quoting)
/// * A subcommand identifier, otherwise
///
/// Arguments before any subcommand identifier are global, and apply to all "cm" invocations.
/// Arguments under a specific subcommand identifier only apply for cm invocations with the
/// appropriate subcommand specified.
///
/// An example config:
///
///     # make the default source dir path be src
///     --source=src
///     # disable quirks-mode detection
///     --quirks=none
///
///     # switch "sections"
///     configure
///     # (the following args will only apply to the configure subcommand)
///     # set a global prefix path dir
///     --prefix-path=/some/absolute/dir
///     # default to make rather than Ninja
///     --generator=Unix Makefiles
///
///     # switch "section" again
///     lit
///     # do not generate a resultdb by default
///     --update-resultdb=false
///
/// Overall, the order in which arguments are evaluated is (later wins):
///
/// * Config file (e.g. ~/.config/cm.rc)
/// * Environment variables (e.g. CM_SRC, CM_BIN, ...)
/// * Command-line options
///
/// Flags are idempotent, and have forms to explicitly specify their defaults (e.g. boolean options
/// generally have an --option=false form), so that setting them in the config file does not limit
/// the user on the command-line (i.e. you can always override your own configured defaults).
///
#[derive(Parser)]
#[command(version, verbatim_doc_comment, infer_subcommands = true)]
#[command(args_override_self = true)]
pub struct Cli {
    #[clap(flatten)]
    pub globals: Globals,
    /// The subcommand
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, ArgsToVec)]
pub struct Globals {
    /// CMake Source Directory
    ///
    /// [default: .]
    #[arg(short, long, env = "CM_SRC", value_hint = ValueHint::DirPath, global = true, help_heading = GLOBAL_HEADING)]
    pub source: Option<PathBuf>,
    /// CMake Binary Directory
    ///
    /// [default: ./build]
    #[arg(short, long, env = "CM_BIN", value_hint = ValueHint::DirPath, global = true, help_heading = GLOBAL_HEADING)]
    pub binary: Option<PathBuf>,
    /// CMake Build Config
    ///
    /// [default: RelWithDebInfo]
    #[arg(short, long, env = "CM_CFG", value_parser = FuzzyParser::new(["Release", "Debug", "RelWithDebInfo", "MinSizeRel"], None), global = true, help_heading = GLOBAL_HEADING)]
    pub config: Option<String>,
    /// Disable quirk mode detection and specify one explicitly
    ///
    /// [default: none]
    #[arg(short, long, env = "CM_QUIRKS", global = true, help_heading = GLOBAL_HEADING)]
    pub quirks: Option<Quirks>,
    /// Perform a dry run, only printing the generated command line
    #[arg(short = '#', long, settable_bool(), global = true, help_heading = GLOBAL_HEADING)]
    pub dry_run: Option<Bool>,
}

impl Globals {
    pub fn final_config(&self) -> &str {
        self.config.as_deref().unwrap_or("RelWithDebInfo")
    }
}

#[derive(clap::ValueEnum, Clone, Copy)]
pub enum Quirks {
    None,
    Llvm,
}

impl AsRef<OsStr> for Quirks {
    fn as_ref(&self) -> &OsStr {
        match self {
            Quirks::None => "none".as_ref(),
            Quirks::Llvm => "llvm".as_ref(),
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
    ///
    /// The "lit" subcommand provides a powerful interface to llvm-lit (and cmake --build, to
    /// implement the -g/--group flag). It optionally ensures that a ResultDB file called
    /// "lit.json" is written to the binary path when tests are run, allowing subsequent runs to
    /// recall which tests failed. With no arguments or flags specifying which tests to run, the
    /// subcommand will run all tests marked as failed in the ResultDB. Repeatedly invoking the
    /// subcommand can thus incrementally "resolve" tests as the ResultDB is updated, removing them
    /// from the list of failing tests until it is empty. This behavior is controlled by the
    /// -u/--update-resulbdb[=<BOOL>] flag which is enabled by default unless a particular subset
    /// of tests is specified (via the -1/--first flag or TESTS arguments). The developer can then
    /// focus on specific failing tests without losing track of the remaining failing tests, and
    /// can record newly passing tests by running the subcommand without specifying a subset.
    ///
    /// The "lit" subcommand will also manage the FILECHECK_OPTS environment variable to make truly
    /// "verbose" lit output easier to achieve.
    #[command(visible_alias = "l")]
    Lit(Lit),
    /// Print shell commands to activate a set of global options
    ///
    /// The "activate" command sets variables for the source directory ("CM_SRC"), binary directory
    /// ("CM_BIN"), configuration ("CM_CFG"), and quirks mode ("CM_QUIRKS"), which are interpreted
    /// as-if they were provided on the command-line. To simplify executing binaries in the binary
    /// directory it also prepends the "bin" subdirectory in the binary path to the "PATH"
    /// environment variable.
    #[command(visible_alias = "a")]
    Activate(Activate),
    /// Print shell commands to deactivate global options set via activate
    ///
    /// The "deactivate" command attempts to undo all of the effects of "activate".
    #[command(visible_alias = "d")]
    Deactivate(Deactivate),
}

#[derive(Args)]
pub struct Configure {
    /// Set CMAKE_PREFIX_PATH
    #[arg(long, overriding_vec())]
    pub prefix_path: Vec<String>,
    /// CMake Generator
    #[arg(short, long, default_value = "Ninja")]
    pub generator: String,
    /// Set BUILD_SHARED_LIBS
    #[arg(long, settable_bool(), default_value_t = true)]
    pub shared_libs: bool,
    /// Enable ASan and UBSan
    #[arg(long, settable_bool())]
    pub san: bool,
    /// Set the preferred linker.
    ///
    /// This is honored on a best-effort basis, and is only currently implemented for
    /// LLVM quirks mode, where the default is to try to use lld or gold if they are available.
    /// This default is intended to work around extremely slow or impossible link steps
    /// for debug builds of LLVM when using the system linker in many environments.
    ///
    /// Specify "default" to explicitly disable automatic linker selection and use the system default.
    #[arg(long, value_parser = FuzzyParser::new(["lld", "gold", "mold", "bfd", "default"], None))]
    pub linker: Option<String>,
    /// Enable expensive checks
    #[arg(long, settable_bool(), help_heading = LLVM_HEADING)]
    pub expensive_checks: bool,
    /// Set LLVM_ENABLE_PROJECTS [default: llvm,clang,lld]
    ///
    /// Accepts comma-separated arguments (e.g. -p bar,baz).
    #[arg(short = 'p', long, overriding_vec(), value_parser = FuzzyParser::new(include!("../values/llvm_all_projects.in"), None), help_heading = LLVM_HEADING)]
    pub enable_projects: Option<Vec<String>>,
    /// Set LLVM_ENABLE_RUNTIMES [default: ""]
    ///
    /// Accepts comma-separated arguments (e.g. -r bar,baz).
    #[arg(short = 'r', long, overriding_vec(), value_parser = FuzzyParser::new(include!("../values/llvm_all_runtimes.in"), None), help_heading = LLVM_HEADING)]
    pub enable_runtimes: Option<Vec<String>>,
    /// Set LLVM_TARGETS_TO_BUILD [default: all]
    ///
    /// Accepts comma-separated arguments (e.g. -t bar,baz).
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
    /// -T/--disable-implicit-native flag.
    #[arg(short, long, overriding_vec(), value_parser = FuzzyParser::new(include!("../values/llvm_all_targets.in"), None), help_heading = LLVM_HEADING)]
    pub targets_to_build: Option<Vec<String>>,
    /// Disable implicit "Native" target in -t/--targets-to-build
    #[arg(short = 'T', long, settable_bool(), help_heading = LLVM_HEADING)]
    pub disable_implicit_native: bool,
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
    #[arg(short, long, settable_bool())]
    pub print_only: bool,
    /// Print a command-line which exports LIT_XFAIL to the tests that would be run
    #[arg(short, long, settable_bool())]
    pub xfail_export: bool,
    /// Update the ResultDB file.
    ///
    /// Defaults to true unless -1/--first or a list of tests (via positional arguments) are
    /// specified.
    ///
    /// Accepts explicit argument via -u/--update-resultdb=true or -u/--update-resultdb=false
    /// and has a shorthand -u/--update-resultdb for the former.
    #[arg(short, long, action = ArgAction::Set, settable_bool(),
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
    /// group. For example, simply "a" is enough to identify "check-all". For all other groups
    /// the full name including the "check-" prefix must be specified.
    #[arg(short, long, group = "select", value_parser = FuzzyParser::new(["all", "llvm", "clang", "lld"], Some("check-")))]
    pub group: Option<String>,
    /// Only consider at most the first failing test in the ResultDB.
    #[arg(short = '1', long, group = "select")]
    pub first: bool,
    /// Be as verbose as possible, asking FileCheck to dump its input and asking llvm-lit to
    /// forward it to stdout
    #[arg(short, long, settable_bool())]
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
