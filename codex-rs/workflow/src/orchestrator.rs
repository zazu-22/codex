use crate::layout::WorkflowLayout;
use crate::manifest::TicketSpec;
use crate::manifest::WorkflowManifest;
use crate::session::SessionLauncher;
use crate::session::SessionRequest;
use crate::state::TicketStatus;
use crate::state::WorkflowState;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_common::CliConfigOverrides;
use std::path::Path;
use std::path::PathBuf;
use textwrap::wrap;

pub struct WorkflowRunOptions {
    pub manifest_path: PathBuf,
    pub artifacts_dir: Option<PathBuf>,
    pub resume: bool,
    pub codex_bin: Option<PathBuf>,
    pub config_overrides: CliConfigOverrides,
    pub worker_model: Option<String>,
    pub reviewer_model: Option<String>,
}

pub struct WorkflowStatusReport {
    pub workflow_name: String,
    pub state_path: PathBuf,
    pub tickets: Vec<crate::state::TicketRunState>,
}

impl WorkflowStatusReport {
    pub fn from_state(state: WorkflowState, state_path: PathBuf) -> Self {
        let tickets = state.tickets.into_values().collect();
        Self {
            workflow_name: state.workflow_name,
            state_path,
            tickets,
        }
    }
}

pub async fn run_workflow(opts: WorkflowRunOptions) -> Result<WorkflowStatusReport> {
    let manifest = WorkflowManifest::load(&opts.manifest_path)?;
    let layout = WorkflowLayout::new(resolve_artifacts_dir(&manifest, &opts.artifacts_dir));
    layout.ensure_root()?;
    let state_path = layout.state_file();

    let mut state = if opts.resume && state_path.exists() {
        let mut state = WorkflowState::load(&state_path)?;
        state.sync_with_manifest(&manifest);
        state
    } else {
        WorkflowState::initialize(&manifest)
    };

    let codex_bin = opts
        .codex_bin
        .clone()
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("codex"));
    let config_flags = opts.config_overrides.raw_overrides.clone();
    let launcher = SessionLauncher::new(codex_bin, config_flags);

    for ticket in &manifest.tickets {
        process_ticket(
            ticket,
            &manifest,
            &layout,
            &mut state,
            &launcher,
            &state_path,
            &opts,
        )
        .await?;
    }

    state.save(&state_path)?;
    Ok(WorkflowStatusReport::from_state(state, state_path))
}

pub fn load_status(
    manifest_path: &Path,
    artifacts_dir: Option<PathBuf>,
) -> Result<Option<WorkflowStatusReport>> {
    let manifest = WorkflowManifest::load(manifest_path)?;
    let layout = WorkflowLayout::new(resolve_artifacts_dir(&manifest, &artifacts_dir));
    let state_path = layout.state_file();
    if !state_path.exists() {
        return Ok(None);
    }
    let state = WorkflowState::load(&state_path)?;
    Ok(Some(WorkflowStatusReport::from_state(state, state_path)))
}

async fn process_ticket(
    ticket: &TicketSpec,
    manifest: &WorkflowManifest,
    layout: &WorkflowLayout,
    state: &mut WorkflowState,
    launcher: &SessionLauncher,
    state_path: &Path,
    opts: &WorkflowRunOptions,
) -> Result<()> {
    let status = match state.ticket(&ticket.id) {
        Some(entry) => entry.status.clone(),
        None => return Ok(()),
    };

    match status {
        TicketStatus::Complete => Ok(()),
        TicketStatus::Failed | TicketStatus::Blocked => Ok(()),
        TicketStatus::NeedsReview | TicketStatus::RunningReview => {
            run_review(ticket, manifest, layout, state, launcher, state_path, opts).await
        }
        _ => {
            run_worker(ticket, manifest, layout, state, launcher, state_path, opts).await?;
            run_review(ticket, manifest, layout, state, launcher, state_path, opts).await
        }
    }
}

async fn run_worker(
    ticket: &TicketSpec,
    manifest: &WorkflowManifest,
    layout: &WorkflowLayout,
    state: &mut WorkflowState,
    launcher: &SessionLauncher,
    state_path: &Path,
    opts: &WorkflowRunOptions,
) -> Result<()> {
    let worker_log = layout.worker_log_path(&ticket.id);
    layout.ensure_ticket_dir(&ticket.id)?;
    let working_dir = ticket.resolved_working_dir(&manifest.manifest_dir());
    if !working_dir.exists() {
        bail!(
            "working directory {} does not exist for ticket {}",
            working_dir.display(),
            ticket.id
        );
    }
    let patch_dir = layout.patch_dir(&ticket.id);
    std::fs::create_dir_all(&patch_dir)
        .with_context(|| format!("failed to create {}", patch_dir.display()))?;
    let prompt = ticket
        .prompt
        .clone()
        .unwrap_or_else(|| build_worker_prompt(manifest, ticket, layout));
    let request = SessionRequest {
        prompt,
        working_dir,
        log_path: worker_log.clone(),
        model: opts.worker_model.clone(),
    };
    if let Some(ticket_state) = state.ticket_mut(&ticket.id) {
        ticket_state.set_worker_log(worker_log.clone());
        ticket_state.mark_running(TicketStatus::RunningWorker);
    }
    state.save(state_path)?;
    let result = launcher.run(request).await?;
    let ticket_state = state
        .ticket_mut(&ticket.id)
        .expect("ticket state exists after worker run");
    if result.success {
        ticket_state.status = TicketStatus::NeedsReview;
        ticket_state.note = Some("Worker completed successfully".to_string());
    } else {
        ticket_state.mark_finished(
            TicketStatus::Failed,
            Some(format!(
                "Worker failed with status {:?}",
                result.status_code
            )),
        );
    }
    state.save(state_path)?;
    Ok(())
}

async fn run_review(
    ticket: &TicketSpec,
    manifest: &WorkflowManifest,
    layout: &WorkflowLayout,
    state: &mut WorkflowState,
    launcher: &SessionLauncher,
    state_path: &Path,
    opts: &WorkflowRunOptions,
) -> Result<()> {
    let status = match state.ticket(&ticket.id) {
        Some(entry) => entry.status.clone(),
        None => return Ok(()),
    };

    if status == TicketStatus::Failed
        || status == TicketStatus::Complete
        || status == TicketStatus::Blocked
    {
        return Ok(());
    }

    if !matches!(
        status,
        TicketStatus::NeedsReview | TicketStatus::RunningReview
    ) {
        return Ok(());
    }

    let review_log = layout.review_log_path(&ticket.id);
    let working_dir = ticket.resolved_working_dir(&manifest.manifest_dir());
    if !working_dir.exists() {
        bail!(
            "working directory {} does not exist for ticket {}",
            working_dir.display(),
            ticket.id
        );
    }
    let prompt = ticket
        .review_prompt
        .clone()
        .unwrap_or_else(|| build_review_prompt(manifest, ticket, layout));
    let request = SessionRequest {
        prompt,
        working_dir,
        log_path: review_log.clone(),
        model: opts
            .reviewer_model
            .clone()
            .or_else(|| opts.worker_model.clone()),
    };

    if let Some(entry) = state.ticket_mut(&ticket.id) {
        entry.set_review_log(review_log.clone());
        entry.mark_running(TicketStatus::RunningReview);
    }
    state.save(state_path)?;

    let result = launcher.run(request).await?;
    let entry = state
        .ticket_mut(&ticket.id)
        .expect("ticket state exists after review");
    if result.success {
        entry.mark_finished(TicketStatus::Complete, Some("Review passed".to_string()));
    } else {
        entry.mark_finished(
            TicketStatus::Failed,
            Some(format!(
                "Review failed with status {:?}",
                result.status_code
            )),
        );
    }
    state.save(state_path)?;
    Ok(())
}

fn build_worker_prompt(
    manifest: &WorkflowManifest,
    ticket: &TicketSpec,
    layout: &WorkflowLayout,
) -> String {
    let mut sections = Vec::new();
    if let Some(overview) = &manifest.overview {
        sections.push(format!("Workflow overview:\n{overview}\n"));
    }
    sections.push(format!("Ticket {}: {}\n", ticket.id, ticket.summary));
    if !ticket.requirements.is_empty() {
        let reqs = ticket
            .requirements
            .iter()
            .map(|req| format!("- {req}"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("Requirements:\n{reqs}\n"));
    }
    let patch_dir = layout.patch_dir(&ticket.id).display().to_string();
    sections.push(format!(
        "Work inside the repository directory and save any generated patches or notes under {patch_dir}. \
        Log your progress clearly."
    ));
    wrap_sections(&sections)
}

fn build_review_prompt(
    manifest: &WorkflowManifest,
    ticket: &TicketSpec,
    layout: &WorkflowLayout,
) -> String {
    let mut sections = Vec::new();
    if let Some(overview) = &manifest.overview {
        sections.push(format!("Workflow overview:\n{overview}\n"));
    }
    sections.push(format!(
        "Review ticket {} ({}) for correctness and completeness.",
        ticket.id, ticket.summary
    ));
    if !ticket.requirements.is_empty() {
        let reqs = ticket
            .requirements
            .iter()
            .map(|req| format!("- {req}"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!(
            "Confirm that the following requirements are satisfied:\n{reqs}\n"
        ));
    }
    let worker_log = layout.worker_log_path(&ticket.id).display().to_string();
    sections.push(format!(
        "Consult the worker log at {worker_log} and ensure all changes are tested. \
        Provide a concise approval or list blocking issues."
    ));
    wrap_sections(&sections)
}

fn wrap_sections(sections: &[String]) -> String {
    let mut result = String::new();
    for section in sections {
        let wrapped = wrap(section, 100);
        for line in wrapped {
            result.push_str(line.trim_end());
            result.push('\n');
        }
        result.push('\n');
    }
    result.trim().to_string()
}

fn resolve_artifacts_dir(manifest: &WorkflowManifest, override_dir: &Option<PathBuf>) -> PathBuf {
    match override_dir {
        Some(dir) => dir.clone(),
        None => manifest
            .manifest_dir()
            .join(".codex")
            .join("workflows")
            .join(manifest.workflow_name()),
    }
}
