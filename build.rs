use clap::{CommandFactory, ValueEnum};
use std::fs::File;
use std::io::Write;

include!("src/cli.rs");

fn main() -> std::io::Result<()> {
    let outdir = std::path::PathBuf::from("gen/");

    std::fs::create_dir_all(&outdir)?;

    let mut cmd = Cli::command();

    for &shell in clap_complete::Shell::value_variants() {
        clap_complete::generate_to(shell, &mut cmd, "cm", &outdir)?;
    }

    let mut buffer: Vec<u8> = Default::default();
    let man = clap_mangen::Man::new(cmd.clone());
    man.render(&mut buffer)?;
    let cmd_name = cmd.get_name();
    std::fs::write(outdir.join(format!("{cmd_name}.1")), &buffer)?;

    for subcmd in cmd.get_subcommands() {
        buffer.clear();
        let man = clap_mangen::Man::new(subcmd.clone());
        man.render(&mut buffer)?;
        let subcmd_name = subcmd.get_name();
        std::fs::write(outdir.join(format!("{cmd_name}-{subcmd_name}.1")), &buffer)?;
    }

    let usage = cmd.render_long_help();
    let mut readme = File::create("README.md")?;
    write!(readme, "# cm\n```text\n{usage}```")?;

    Ok(())
}
