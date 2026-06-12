#[cfg(feature = "tui")]
use crate::commands::Result;

#[cfg(feature = "tui")]
pub async fn run() -> Result<()> {
    println!("🚧 TUI mode is not yet implemented");
    println!("This will launch an interactive dashboard with:");
    println!("  • Live connector status");
    println!("  • Search interface");
    println!("  • Configuration management");
    println!("  • Real-time data streaming");
    println!();
    println!("For now, use the CLI commands:");
    println!("  rzn-tools list");
    println!("  rzn-tools search <connector> <query>");
    println!("  rzn-tools get <connector> <id>");
    println!("  rzn-tools config show");

    Ok(())
}

#[cfg(not(feature = "tui"))]
pub async fn run() -> Result<()> {
    Err(CommandError::InvalidConfig(
        "TUI feature not enabled. Compile with --features tui".to_string(),
    ))
}
