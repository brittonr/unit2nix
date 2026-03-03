mod cargo;
mod cli;
mod merge;
mod metadata;
mod output;
mod overrides;
mod prefetch;
mod run;
mod source;
mod unit_graph;

use anyhow::Result;
use clap::Parser;

use cli::Cli;

fn main() -> Result<()> {
    run::run(Cli::parse())
}
