use unit2nix::cli::Cli;
use unit2nix::run;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    run::run(&Cli::parse())
}
