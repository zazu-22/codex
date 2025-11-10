use anyhow::Context;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct WorkflowManifest {
    #[serde(skip)]
    pub source_path: PathBuf,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub tickets: Vec<TicketSpec>,
}

impl WorkflowManifest {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read workflow manifest {}", path.display()))?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut manifest: WorkflowManifest = match ext.as_str() {
            "yml" | "yaml" => serde_yaml::from_str(&contents).context("parse workflow manifest")?,
            "toml" | "tml" => toml::from_str(&contents).context("parse workflow manifest")?,
            _ => serde_yaml::from_str(&contents)
                .or_else(|_| toml::from_str(&contents))
                .context("parse workflow manifest (yaml or toml)")?,
        };
        manifest.source_path = path.to_path_buf();
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.tickets.is_empty() {
            anyhow::bail!("workflow manifest must contain at least one ticket");
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for ticket in &self.tickets {
            if !seen.insert(ticket.id.as_str()) {
                anyhow::bail!("duplicate ticket id {}", ticket.id);
            }
        }
        Ok(())
    }

    pub fn manifest_dir(&self) -> PathBuf {
        self.source_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn workflow_name(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        self.source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("workflow")
            .to_string()
    }
}

#[derive(Debug, Deserialize)]
pub struct TicketSpec {
    pub id: String,
    pub summary: String,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub review_prompt: Option<String>,
}

impl TicketSpec {
    pub fn resolved_working_dir(&self, manifest_dir: &Path) -> PathBuf {
        match &self.working_dir {
            Some(path) if path.is_absolute() => path.clone(),
            Some(path) => manifest_dir.join(path),
            None => manifest_dir.to_path_buf(),
        }
    }
}

impl Default for WorkflowManifest {
    fn default() -> Self {
        Self {
            source_path: PathBuf::new(),
            name: None,
            overview: None,
            tickets: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_yaml_manifest_and_resolves_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manifest_path = dir.path().join("demo.yaml");
        let contents = r#"
name: demo
overview: Demo workflow
tickets:
  - id: T1
    summary: First ticket
    requirements:
      - Add tests
      - Update docs
    working_dir: .
  - id: T2
    summary: Second ticket
"#;
        fs::write(&manifest_path, contents).expect("write manifest");
        let manifest = WorkflowManifest::load(&manifest_path).expect("load");
        assert_eq!(manifest.workflow_name(), "demo");
        assert_eq!(manifest.tickets.len(), 2);
        let ticket = &manifest.tickets[0];
        let resolved = ticket.resolved_working_dir(manifest.manifest_dir().as_path());
        assert_eq!(resolved, manifest.manifest_dir());
    }
}
