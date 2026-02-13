use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::process::Command;
use std::env;

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Development tasks for the project", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build release binary and install to /usr/local/bin (requires sudo)
    Install,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Install => {
            install()?;
        }
    }

    Ok(())
}

fn install() -> Result<()> {
    // Locate the project root relative to the cargo manifest of the xtask package
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?;
    let workspace_root = std::path::Path::new(&manifest_dir).parent().context("Failed to find workspace root")?;
    
    // Change directory to workspace root
    std::env::set_current_dir(workspace_root).context("Failed to change directory to workspace root")?;

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    
    println!("Building release binary...");
    let status = Command::new(&cargo)
        .arg("build")
        .arg("--release")
        .status()
        .context("Failed to run cargo build")?;

    if !status.success() {
        anyhow::bail!("Cargo build failed");
    }

    println!("Installing to /usr/local/bin (sudo required)...");
    let status = Command::new("sudo")
        .arg("cp")
        .arg("target/release/cfg2hcl")
        .arg("/usr/local/bin/")
        .status()
        .context("Failed to run sudo cp")?;

    if !status.success() {
        anyhow::bail!("Installation failed");
    }

    println!("Successfully installed cfg2hcl to /usr/local/bin");
    Ok(())
}
