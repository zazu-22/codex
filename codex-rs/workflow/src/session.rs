use anyhow::Context;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct SessionLauncher {
    codex_bin: PathBuf,
    config_overrides: Vec<String>,
}

impl SessionLauncher {
    pub fn new(codex_bin: PathBuf, config_overrides: Vec<String>) -> Self {
        Self {
            codex_bin,
            config_overrides,
        }
    }

    pub async fn run(&self, request: SessionRequest) -> anyhow::Result<SessionResult> {
        let mut cmd = Command::new(&self.codex_bin);
        cmd.arg("exec");
        for override_flag in &self.config_overrides {
            cmd.arg("-c");
            cmd.arg(override_flag);
        }
        cmd.arg("--skip-git-repo-check");
        if let Some(model) = &request.model {
            cmd.arg("-m");
            cmd.arg(model);
        }
        cmd.arg("-C");
        cmd.arg(&request.working_dir);
        cmd.arg(&request.prompt);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd
            .output()
            .await
            .with_context(|| format!("failed to run {}", self.codex_bin.display()))?;

        write_log(&request.log_path, &request.prompt, &output)?;

        let status_code = output.status.code();
        Ok(SessionResult {
            success: output.status.success(),
            status_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn write_log(log_path: &Path, prompt: &str, output: &std::process::Output) -> anyhow::Result<()> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = std::fs::File::create(log_path)
        .with_context(|| format!("failed to create {}", log_path.display()))?;
    use std::io::Write;
    writeln!(file, "# Prompt")?;
    writeln!(file, "{prompt}")?;
    writeln!(file)?;
    writeln!(file, "# Exit Status: {:?}", output.status.code())?;
    writeln!(file)?;
    writeln!(file, "## STDOUT")?;
    file.write_all(&output.stdout)?;
    if !output.stdout.ends_with(b"\n") {
        writeln!(file)?;
    }
    writeln!(file)?;
    writeln!(file, "## STDERR")?;
    file.write_all(&output.stderr)?;
    writeln!(file)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct SessionRequest {
    pub prompt: String,
    pub working_dir: PathBuf,
    pub log_path: PathBuf,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionResult {
    #[allow(dead_code)]
    pub success: bool,
    pub status_code: Option<i32>,
    #[allow(dead_code)]
    pub stdout: String,
    #[allow(dead_code)]
    pub stderr: String,
}
