use super::types::{BashOutput, ExecutorError};
use async_stream::stream;
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

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
                    let err = ExecutorError::SpawnFailed {
                        command: command.clone(),
                        source: e,
                    };
                    tracing::error!(error = %err, "Bash executor error");
                    yield BashOutput::Error { message: err.to_string() };
                    return;
                }
            };

            // Get stdout and stderr handles
            let stdout = match child.stdout.take() {
                Some(s) => s,
                None => {
                    let err = ExecutorError::StdioCaptureFailed {
                        stream: "stdout",
                        command: command.clone(),
                    };
                    tracing::error!(error = %err, "Bash executor error");
                    yield BashOutput::Error { message: err.to_string() };
                    return;
                }
            };
            let stderr = match child.stderr.take() {
                Some(s) => s,
                None => {
                    let err = ExecutorError::StdioCaptureFailed {
                        stream: "stderr",
                        command: command.clone(),
                    };
                    tracing::error!(error = %err, "Bash executor error");
                    yield BashOutput::Error { message: err.to_string() };
                    return;
                }
            };

            // Capture stderr in background task to prevent pipe buffer blocking
            let stderr_buffer: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let stderr_buf = stderr_buffer.clone();
            tokio::spawn(async move {
                let mut stderr_reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = stderr_reader.next_line().await {
                    let mut buf = stderr_buf.lock().await;
                    if buf.len() < 100 {
                        buf.push(line);
                    }
                }
            });

            // Stream stdout lines
            let mut stdout_reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = stdout_reader.next_line().await {
                yield BashOutput::Stdout { line };
            }

            // Yield captured stderr lines
            let stderr_lines = stderr_buffer.lock().await;
            if !stderr_lines.is_empty() {
                tracing::debug!(
                    stderr_line_count = stderr_lines.len(),
                    command = %command,
                    "Bash process produced stderr output"
                );
                for line in stderr_lines.iter() {
                    yield BashOutput::Stderr { line: line.clone() };
                }
            }
            drop(stderr_lines);

            // Wait for process to complete and get exit code
            match child.wait().await {
                Ok(status) => {
                    let code = status.code().unwrap_or(-1);
                    tracing::debug!(exit_code = code, command = %command, "Bash process exited");
                    yield BashOutput::Exit { code };
                }
                Err(e) => {
                    let err = ExecutorError::WaitFailed {
                        command: command.clone(),
                        source: e,
                    };
                    tracing::error!(error = %err, "Bash executor error");
                    yield BashOutput::Error { message: err.to_string() };
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
