use std::{env, io::IsTerminal};

use anyhow::{bail, Result};
use newhatchctl::{app, docker::Docker, project::Project};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [] => {}
        [arg] if arg == "--help" || arg == "-h" => {
            println!("newhatchctl — local newhatch control panel\n\nUsage: newhatchctl\n\nRun in the newhatch directory, or set NEWHATCH_DIR.\nKeyboard: arrows, Tab, Enter, Esc; q/Ctrl+C to exit.\nRequires Docker with Compose v2 and an interactive terminal.");
            return Ok(());
        }
        [arg] if arg == "--version" || arg == "-V" => {
            println!("newhatchctl {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => bail!("Usage: newhatchctl [--help | --version]"),
    }
    let project = Project::resolve(&env::current_dir()?, env::var_os("NEWHATCH_DIR").as_deref())?;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("newhatchctl requires an interactive terminal");
    }
    app::run(Docker::new(project))
}
