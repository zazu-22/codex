use anyhow::Context;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct WorkflowLayout {
    root: PathBuf,
}

impl WorkflowLayout {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_root(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))
    }

    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }

    pub fn ticket_dir(&self, ticket_id: &str) -> PathBuf {
        self.root.join(format!("ticket-{}", sanitize(ticket_id)))
    }

    pub fn ensure_ticket_dir(&self, ticket_id: &str) -> anyhow::Result<PathBuf> {
        let dir = self.ticket_dir(ticket_id);
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        Ok(dir)
    }

    pub fn worker_log_path(&self, ticket_id: &str) -> PathBuf {
        self.ticket_dir(ticket_id).join("worker.log")
    }

    pub fn review_log_path(&self, ticket_id: &str) -> PathBuf {
        self.ticket_dir(ticket_id).join("review.log")
    }

    pub fn patch_dir(&self, ticket_id: &str) -> PathBuf {
        self.ticket_dir(ticket_id).join("patches")
    }
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_dirs_are_sanitized() {
        let layout = WorkflowLayout::new(PathBuf::from("/tmp/workflow"));
        let dir = layout.ticket_dir("ABC/123");
        assert!(dir.ends_with("ticket-ABC_123"));
        assert_eq!(
            layout.worker_log_path("hello world"),
            PathBuf::from("/tmp/workflow/ticket-hello_world/worker.log")
        );
    }
}
