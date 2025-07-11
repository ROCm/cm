Welcome, and thanks for the interest in improving `cm`!

# Principles

The guiding principles of the project are (in order of priority, most to least):

* Correctness
* Consistency/Clarity
* Interactivity

(Almost) nobody wants to spend their time learning the quirks of their build
and test systems. And yet...

* If they are fundamentally broken/wrong it is impossible not to.
* If they are inconsistent or confusing it is difficult not to.
* If they are needlessly verbose, or give no feedback/interactive help from the
command-line then it is frustrating to have to.

# Util

Several maintenance tasks have scripts in the `util/` directory to act as
reminders/shortcuts. The most generally useful are `util/test-*` for managing
the `trycmd`-based testsuite (see below), and `util/smoke` for emulating the
same checks as the GitHub CI.

# Tests

There isn't great test coverage, but try to include at least some testing of
all new changes. The workflow for developing tests with `trycmd` is described
at https://docs.rs/trycmd/latest/trycmd/#workflow and in general try to prefer
`*.toml`-based tests so that `util/test-overwrite` is all that's required to
reflect changes in all tests. It is then easy to review the diff to make sure
changes are expected.

# Code Structure

The root of the app is `src/main.rs`, containing the entry-point `main` which
does only enough to call the function `cm::cm` and connect its output to the
command-line user. This keeps the implementation of `cm::cm` as "pure" as
possible.

The function `cm::cm` is defined in `src/cm.rs` and contains the actual logic
for the app. It essentially implements the "plan-execute" pattern, first
creating a (mostly[^1]) non-destructive "plan" of shell command-lines needed to
implement the request, and then executing them sequentially. This approach
makes implementing the "dry-run" option `-#` trivial, as we just format and
print the commands rather than execute them.

The command-line interface is declared separately in `src/cli.rs` to make it
easier for `build.rs` to generate `man` pages, autocompletion scripts, etc.

# Tools

The choice of Rust was largely due to the package ecosystem which does most of
the heavy lifting for all the "small" touches which make the tool useful in an
interactive context. Namely:

* Declarative command-line interface specification with
[clap](https://github.com/clap-rs/clap).
  * First class subcommand support a-la `git`.
  * Auto-generated autocomplete scripts for several common shells, including
  Bash, Zsh, and Fish.
  * Auto-generated `man(1)` pages for the supercommand `cm(1)` and each
  sub-command, e.g. `cm-activate(1)`.
* Easy "sandboxed" black-box command-line testing with
[trycmd](https://docs.rs/trycmd/latest/trycmd/).


[^1]: We do things which technically could be considered "destructive" in some
scenarios, like running commands to check for available compiler options, but
the goal is to be as non-destructive as possible.
