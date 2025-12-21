//! Simple test to verify Claude Code streaming works
//!
//! Run with: cargo run --example claude_stream_test

use futures::StreamExt;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Find claude binary
    let claude_path = dirs::home_dir()
        .map(|h| h.join(".claude/local/claude"))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "claude".to_string());

    println!("Using claude binary: {}", claude_path);

    // Build command
    let mut cmd = Command::new(&claude_path);
    cmd.args([
        "--output-format", "stream-json",
        "--include-partial-messages",
        "--verbose",
        "--print",
        "--model", "haiku",
        "--", "just say hello"
    ]);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.current_dir("/tmp");

    println!("Launching claude...");
    let mut child = cmd.spawn()?;

    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");

    // Spawn stderr reader
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[stderr] {}", line);
        }
    });

    // Read stdout
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    println!("Reading stream...");
    let mut count = 0;
    while let Ok(Some(line)) = lines.next_line().await {
        count += 1;
        println!("[{}] {}", count, line);

        // Parse as JSON to see what type
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(typ) = json.get("type").and_then(|v| v.as_str()) {
                println!("    -> type: {}", typ);
            }
        }
    }

    println!("Stream ended after {} lines", count);

    // Wait for child
    let status = child.wait().await?;
    println!("Claude exited with: {}", status);

    stderr_handle.abort();

    Ok(())
}
