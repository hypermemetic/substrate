use super::types::BashOutput;
use async_stream::stream;
use futures::Stream;
use std::pin::Pin;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Core bash executor - can be used programmatically without RPC
#[derive(Clone)]
pub struct BashExecutor;

impl BashExecutor {
    pub fn new() -> Self {
        Self
    }

    /// Execute a bash command and stream the output
    ///
    /// This is the core business logic - completely independent of RPC.
    /// Returns a stream of BashOutput items.
    pub async fn execute(
        &self,
        command: &str,
    ) -> Pin<Box<dyn Stream<Item = BashOutput> + Send + 'static>> {
        let command = command.to_string();

        Box::pin(stream! {
            // Spawn the bash process
            let mut child = match Command::new("bash")
                .arg("-c")
                .arg(&command)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    // If we can't spawn, yield an error through BashOutput
                    // Note: We can't yield BashError here since the stream type is BashOutput
                    // In a real system, we might want to make this an enum or handle differently
                    eprintln!("Failed to spawn bash: {}", e);
                    return;
                }
            };

            // Get stdout and stderr handles
            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let stderr = child.stderr.take().expect("Failed to capture stderr");

            // Create buffered readers
            let mut stdout_reader = BufReader::new(stdout).lines();
            let mut stderr_reader = BufReader::new(stderr).lines();

            // Read lines from both stdout and stderr
            // Note: This is a simple implementation that reads stdout first, then stderr
            // A more sophisticated version would interleave them as they arrive
            while let Ok(Some(line)) = stdout_reader.next_line().await {
                yield BashOutput::Stdout { line };
            }

            while let Ok(Some(line)) = stderr_reader.next_line().await {
                yield BashOutput::Stderr { line };
            }

            // Wait for process to complete and get exit code
            match child.wait().await {
                Ok(status) => {
                    let code = status.code().unwrap_or(-1);
                    yield BashOutput::Exit { code };
                }
                Err(e) => {
                    eprintln!("Failed to wait for child: {}", e);
                    yield BashOutput::Exit { code: -1 };
                }
            }
        })
    }

    /// Execute a command and collect all output (convenience method for testing)
    pub async fn execute_collect(&self, command: &str) -> Vec<BashOutput> {
        use futures::StreamExt;

        let mut results = Vec::new();
        let mut stream = self.execute(command).await;

        while let Some(output) = stream.next().await {
            results.push(output);
        }

        results
    }
}

impl Default for BashExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_simple_command() {
        let executor = BashExecutor::new();
        let outputs = executor.execute_collect("echo 'hello world'").await;

        // Should have stdout line + exit code
        assert!(outputs.len() >= 2);

        // Check for stdout
        match &outputs[0] {
            BashOutput::Stdout { line } => assert_eq!(line, "hello world"),
            _ => panic!("Expected stdout"),
        }

        // Check for successful exit
        match outputs.last().unwrap() {
            BashOutput::Exit { code } => assert_eq!(*code, 0),
            _ => panic!("Expected exit"),
        }
    }

    #[tokio::test]
    async fn test_execute_stderr() {
        let executor = BashExecutor::new();
        let outputs = executor
            .execute_collect("echo 'error' >&2")
            .await;

        // Should have stderr line + exit code
        assert!(outputs.len() >= 2);

        // Check for stderr
        let has_stderr = outputs.iter().any(|o| matches!(o, BashOutput::Stderr { .. }));
        assert!(has_stderr);
    }

    #[tokio::test]
    async fn test_execute_exit_code() {
        let executor = BashExecutor::new();
        let outputs = executor.execute_collect("exit 42").await;

        // Check for exit code 42
        match outputs.last().unwrap() {
            BashOutput::Exit { code } => assert_eq!(*code, 42),
            _ => panic!("Expected exit"),
        }
    }
}
