use anyhow::Result;
use clap::{Args, Subcommand};
use codex_common::CliConfigOverrides;
use codex_workflow::{load_status, run_workflow, WorkflowRunOptions, WorkflowStatusReport};
use std::path::PathBuf;

use crate::prepend_config_flags;

#[derive(Debug, Args)]
pub struct WorkflowCli {
    #[command(subcommand)]
    pub action: WorkflowSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowSubcommand {
    /// Run an orchestrated workflow based on a manifest file.
    Run(WorkflowRunArgs),
    /// Display the current status of a workflow.
    Status(WorkflowStatusArgs),
}

#[derive(Debug, Args)]
pub struct WorkflowRunArgs {
    /// Path to the workflow manifest (YAML or TOML).
    #[arg(value_name = "MANIFEST")]
    pub manifest: PathBuf,

    /// Directory to store workflow artifacts (logs, patches, state.json).
    #[arg(long = "artifacts-dir", value_name = "DIR")]
    pub artifacts_dir: Option<PathBuf>,

    /// Resume from a previously saved workflow state if available.
    #[arg(long)]
    pub resume: bool,

    /// Override the Codex binary path (defaults to the current executable).
    #[arg(long = "codex-bin", value_name = "PATH")]
    pub codex_bin: Option<PathBuf>,

    /// Optional worker model override passed to codex exec.
    #[arg(long = "worker-model", value_name = "MODEL")]
    pub worker_model: Option<String>,

    /// Optional reviewer model override passed to codex exec.
    #[arg(long = "reviewer-model", value_name = "MODEL")]
    pub reviewer_model: Option<String>,

    #[clap(flatten)]
    pub config_overrides: CliConfigOverrides,
}

#[derive(Debug, Args)]
pub struct WorkflowStatusArgs {
    /// Path to the workflow manifest (YAML or TOML).
    #[arg(value_name = "MANIFEST")]
    pub manifest: PathBuf,

    /// Directory that stores workflow artifacts. If omitted, defaults to
    /// `.codex/workflows/<workflow-name>` next to the manifest.
    #[arg(long = "artifacts-dir", value_name = "DIR")]
    pub artifacts_dir: Option<PathBuf>,
}

pub async fn execute(cli: WorkflowCli, root_overrides: CliConfigOverrides) -> Result<()> {
    match cli.action {
        WorkflowSubcommand::Run(mut run_args) => {
            prepend_config_flags(&mut run_args.config_overrides, root_overrides);
            run(run_args).await
        }
        WorkflowSubcommand::Status(status_args) => status(status_args),
    }
}

async fn run(args: WorkflowRunArgs) -> Result<()> {
    let options = WorkflowRunOptions {
        manifest_path: args.manifest,
        artifacts_dir: args.artifacts_dir,
        resume: args.resume,
        codex_bin: args.codex_bin,
        config_overrides: args.config_overrides,
        worker_model: args.worker_model,
        reviewer_model: args.reviewer_model,
    };
    let report = run_workflow(options).await?;
    print_report(&report);
    Ok(())
}

fn status(args: WorkflowStatusArgs) -> Result<()> {
    match load_status(&args.manifest, args.artifacts_dir) {
        Ok(Some(report)) => {
            print_report(&report);
            Ok(())
        }
        Ok(None) => {
            println!(
                "No workflow state found for manifest {}",
                args.manifest.display()
            );
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn print_report(report: &WorkflowStatusReport) {
    println!("Workflow: {}", report.workflow_name);
    println!("State file: {}", report.state_path.display());
    for ticket in &report.tickets {
        println!(
            "- {:<12} {:<15} {}",
            ticket.ticket_id,
            format!("{:?}", ticket.status),
            ticket
                .note
                .as_deref()
                .unwrap_or("No status note recorded yet.")
        );
        if let Some(worker_log) = &ticket.worker_log {
            println!("    worker log: {}", worker_log.display());
        }
        if let Some(review_log) = &ticket.review_log {
            println!("    review log: {}", review_log.display());
        }
    }
}
