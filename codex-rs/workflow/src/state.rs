use crate::manifest::WorkflowManifest;
use anyhow::Context;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub workflow_name: String,
    pub tickets: BTreeMap<String, TicketRunState>,
}

impl WorkflowState {
    pub fn initialize(manifest: &WorkflowManifest) -> Self {
        let tickets = manifest
            .tickets
            .iter()
            .map(|ticket| {
                (
                    ticket.id.clone(),
                    TicketRunState {
                        ticket_id: ticket.id.clone(),
                        status: TicketStatus::Pending,
                        worker_log: None,
                        review_log: None,
                        note: None,
                        started_at: None,
                        finished_at: None,
                    },
                )
            })
            .collect();

        Self {
            workflow_name: manifest.workflow_name(),
            tickets,
        }
    }

    pub fn sync_with_manifest(&mut self, manifest: &WorkflowManifest) {
        for ticket in &manifest.tickets {
            self
                .tickets
                .entry(ticket.id.clone())
                .or_insert_with(|| TicketRunState {
                    ticket_id: ticket.id.clone(),
                    status: TicketStatus::Pending,
                    worker_log: None,
                    review_log: None,
                    note: None,
                    started_at: None,
                    finished_at: None,
                });
        }
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data = fs::read_to_string(path)
            .with_context(|| format!("failed to read workflow state {}", path.display()))?;
        let state: WorkflowState =
            serde_json::from_str(&data).context("parse workflow state json")?;
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let tmp_path = tmp_path(path);
        let data = serde_json::to_vec_pretty(self)?;
        fs::write(&tmp_path, data)
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        fs::rename(&tmp_path, path)
            .with_context(|| format!("failed to persist {}", path.display()))?;
        Ok(())
    }

    pub fn ticket(&self, ticket_id: &str) -> Option<&TicketRunState> {
        self.tickets.get(ticket_id)
    }

    pub fn ticket_mut(&mut self, ticket_id: &str) -> Option<&mut TicketRunState> {
        self.tickets.get_mut(ticket_id)
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let mut file_name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    file_name.push(".tmp");
    tmp.set_file_name(file_name);
    tmp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketRunState {
    pub ticket_id: String,
    pub status: TicketStatus,
    pub worker_log: Option<PathBuf>,
    pub review_log: Option<PathBuf>,
    pub note: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl TicketRunState {
    pub fn mark_running(&mut self, status: TicketStatus) {
        self.status = status;
        if self.started_at.is_none() {
            self.started_at = Some(Utc::now());
        }
        self.note = None;
    }

    pub fn mark_finished(&mut self, status: TicketStatus, note: Option<String>) {
        self.status = status;
        self.note = note;
        self.finished_at = Some(Utc::now());
    }

    pub fn set_worker_log(&mut self, log_path: PathBuf) {
        self.worker_log = Some(log_path);
    }

    pub fn set_review_log(&mut self, log_path: PathBuf) {
        self.review_log = Some(log_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::TicketSpec;
    use crate::manifest::WorkflowManifest;
    use std::path::PathBuf;

    #[test]
    fn initializes_state_with_pending_tickets() {
        let manifest = WorkflowManifest {
            source_path: PathBuf::from("workflow.yaml"),
            name: Some("demo".into()),
            overview: None,
            tickets: vec![
                TicketSpec {
                    id: "A".into(),
                    summary: "Ticket A".into(),
                    requirements: vec![],
                    working_dir: None,
                    prompt: None,
                    review_prompt: None,
                },
                TicketSpec {
                    id: "B".into(),
                    summary: "Ticket B".into(),
                    requirements: vec![],
                    working_dir: None,
                    prompt: None,
                    review_prompt: None,
                },
            ],
        };

        let state = WorkflowState::initialize(&manifest);
        assert_eq!(state.tickets.len(), 2);
        assert!(
            state
                .tickets
                .values()
                .all(|ticket| ticket.status == TicketStatus::Pending)
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TicketStatus {
    Pending,
    RunningWorker,
    NeedsReview,
    RunningReview,
    Complete,
    Failed,
    Blocked,
}
