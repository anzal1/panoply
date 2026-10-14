use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use panoply_core::{CancelToken, JobEvent, JobSpec};

use crate::{CapabilityGuard, Module, ModuleError, RunContext};

/// What a finished job produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobOutcome {
    /// Files written.
    pub outputs: Vec<PathBuf>,
    /// Report lines from read-only modules.
    pub notes: Vec<String>,
}

/// Registered modules, looked up by id.
#[derive(Default)]
pub struct Registry {
    modules: BTreeMap<String, Arc<dyn Module>>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry with every built-in module.
    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        for m in crate::builtin::all() {
            r.register(m).expect("built-in module ids are unique");
        }
        r
    }

    /// Add a module. Ids are unique.
    pub fn register(&mut self, module: Arc<dyn Module>) -> Result<(), ModuleError> {
        let id = module.manifest().module.id.clone();
        if self.modules.contains_key(&id) {
            return Err(ModuleError::Duplicate(id));
        }
        self.modules.insert(id, module);
        Ok(())
    }

    /// Find a module by id.
    pub fn get(&self, id: &str) -> Option<&Arc<dyn Module>> {
        self.modules.get(id)
    }

    /// Every module, sorted by id.
    pub fn list(&self) -> impl Iterator<Item = &Arc<dyn Module>> {
        self.modules.values()
    }

    /// Number of modules.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// True when nothing is registered.
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Run a job end to end: build the capability guard from the manifest and the chosen
    /// inputs, run the module, and report `Done` or `Failed` through `sink`.
    pub fn run_job(
        &self,
        spec: &JobSpec,
        out_dir: Option<&std::path::Path>,
        cancel: CancelToken,
        sink: impl Fn(JobEvent) + Send + Sync + Clone + 'static,
    ) -> Result<JobOutcome, ModuleError> {
        let result = self.run_job_inner(spec, out_dir, cancel, sink.clone());
        match &result {
            Ok(o) => sink(JobEvent::Done {
                outputs: o.outputs.clone(),
            }),
            Err(e) => sink(e.to_event()),
        }
        result
    }

    fn run_job_inner(
        &self,
        spec: &JobSpec,
        out_dir: Option<&std::path::Path>,
        cancel: CancelToken,
        sink: impl Fn(JobEvent) + Send + Sync + 'static,
    ) -> Result<JobOutcome, ModuleError> {
        let module = self
            .get(&spec.module_id)
            .ok_or_else(|| ModuleError::UnknownModule(spec.module_id.clone()))?;
        let caps = &module.manifest().capabilities;
        let guard = match caps.files {
            panoply_manifest::FilesAccess::Chosen => {
                CapabilityGuard::new(&spec.inputs, out_dir, caps.network)?
            }
            panoply_manifest::FilesAccess::None => {
                CapabilityGuard::new(&[], out_dir, caps.network)?
            }
        };
        let ctx = RunContext::new(guard)
            .with_cancel(cancel)
            .with_progress(sink);
        // The manifest's network line is the scope for every download this job could start, and
        // a refusal from the consent gate is reported as what it is, not as a vague failure.
        panoply_core::assets::clear_refusal();
        let ran = panoply_core::assets::with_network_scope(caps.network, || {
            module.run(&ctx, &spec.inputs, &spec.options)
        });
        let outputs = match ran {
            Ok(o) => o,
            Err(e @ ModuleError::Cancelled) => return Err(e),
            Err(e) => {
                return Err(match panoply_core::assets::take_refusal() {
                    Some(refusal) => ModuleError::Consent(refusal),
                    None => e,
                });
            }
        };
        Ok(JobOutcome {
            outputs,
            notes: ctx.take_notes(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn builtins_include_hash() {
        let r = Registry::with_builtins();
        assert!(r.get("files.hash").is_some());
        assert!(!r.is_empty());
        assert!(r.get("nope.nothing").is_none());
    }

    #[test]
    fn duplicates_are_refused() {
        let mut r = Registry::with_builtins();
        let again = crate::builtin::all().remove(0);
        assert!(matches!(r.register(again), Err(ModuleError::Duplicate(_))));
    }

    #[test]
    fn list_is_sorted_by_id() {
        let r = Registry::with_builtins();
        let ids: Vec<_> = r.list().map(|m| m.manifest().module.id.clone()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn unknown_module_fails_with_failed_event() {
        let r = Registry::with_builtins();
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        let spec = JobSpec {
            module_id: "no.such".into(),
            inputs: vec![],
            options: Default::default(),
        };
        let res = r.run_job(&spec, None, CancelToken::new(), move |e| {
            e2.lock().unwrap().push(e)
        });
        assert!(matches!(res, Err(ModuleError::UnknownModule(_))));
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(JobEvent::Failed { .. })
        ));
    }
}
