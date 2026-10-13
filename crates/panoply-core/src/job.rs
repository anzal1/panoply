//! Job types shared by the CLI, MCP, and every native UI.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Identifies one job in history and in events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub u64);

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "job-{}", self.0)
    }
}

/// What to run: a module, the files chosen for it, and its options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSpec {
    /// Module id, for example `files.hash`.
    pub module_id: String,
    /// Input files the user chose. Nothing else is readable.
    pub inputs: Vec<PathBuf>,
    /// Options keyed by the names declared in the module manifest.
    #[serde(default)]
    pub options: serde_json::Value,
}

/// Events a running job reports. The UI renders these, never raw errors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JobEvent {
    /// Work is moving. `fraction` is 0.0 to 1.0.
    Progress {
        /// Completed share of the job.
        fraction: f32,
        /// Short human label such as "Hashing report.pdf".
        label: String,
    },
    /// The job finished and wrote these files.
    Done {
        /// Output paths, possibly empty for read-only modules.
        outputs: Vec<PathBuf>,
    },
    /// The job failed. Always a plain message plus a next step.
    Failed {
        /// What went wrong in plain language.
        plain_message: String,
        /// What the user can do next.
        next_step: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spec_round_trips() {
        let spec = JobSpec {
            module_id: "files.hash".into(),
            inputs: vec![PathBuf::from("/tmp/a.bin")],
            options: json!({"algo": "sha256"}),
        };
        let s = serde_json::to_string(&spec).unwrap();
        let back: JobSpec = serde_json::from_str(&s).unwrap();
        assert_eq!(spec, back);
    }

    #[test]
    fn spec_options_default_to_null() {
        let spec: JobSpec = serde_json::from_str(r#"{"module_id":"x","inputs":[]}"#).unwrap();
        assert!(spec.options.is_null());
    }

    #[test]
    fn events_are_tagged() {
        let e = JobEvent::Failed {
            plain_message: "This PDF is password-protected.".into(),
            next_step: "Enter the password or try Unlock PDF.".into(),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["event"], "failed");
        assert_eq!(v["next_step"], "Enter the password or try Unlock PDF.");
    }

    #[test]
    fn job_id_displays() {
        assert_eq!(JobId(7).to_string(), "job-7");
    }
}
