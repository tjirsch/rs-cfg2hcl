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
    /// Build release binary and install to ~/.local/bin (no sudo required)
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

    // Get home directory and construct ~/.local/bin path
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let install_dir = std::path::Path::new(&home).join(".local").join("bin");
    
    // Create directory if it doesn't exist
    std::fs::create_dir_all(&install_dir)
        .context(format!("Failed to create directory: {}", install_dir.display()))?;
    
    let install_path = install_dir.join("cfg2hcl");
    
    println!("Installing to {}...", install_dir.display());
    std::fs::copy("target/release/cfg2hcl", &install_path)
        .context(format!("Failed to copy binary to {}", install_path.display()))?;
    
    // Make executable (in case permissions are wrong)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&install_path, std::fs::Permissions::from_mode(0o755))
            .context("Failed to set executable permissions")?;
    }
    
    println!("Successfully installed cfg2hcl to {}", install_dir.display());
    println!("Make sure {} is in your PATH", install_dir.display());
    Ok(())
}
