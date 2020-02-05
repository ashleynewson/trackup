use ::serde::{Serialize,Deserialize};
use ::chrono::DateTime;
use ::chrono::offset::Local;
use ::std::collections::{HashSet,HashMap};
use ::std::fs::File;
use ::std::path::{Path,PathBuf};
use crate::control::{Job,Manifest};

#[derive(Eq,PartialEq,Clone,Serialize,Deserialize)]
pub enum Health {
    Setup,
    Partial,
    Finishing,
    Success,
    Failure,
}

/// Tracks the files and state of a snapshot.
#[derive(Clone,Serialize,Deserialize)]
pub struct State {
    /// A unique name (based on timestamp). Used for filing.
    name: String,
    /// Directory which stores multiple backups.
    store_path: Option<PathBuf>,
    /// Path which always points to current state
    #[serde(skip)]
    path: Option<PathBuf>,
    /// Path which always points to last successful state
    #[serde(skip)]
    success_path: Option<PathBuf>,
    /// Path which points to this backup's state
    permanent_path: Option<PathBuf>,
    /// Path to the parent state if one exists.
    ///
    /// Note that this should be absolute or relative to PWD.
    parent_path: Option<PathBuf>,
    #[serde(skip)]
    parent: Option<Box<State>>,
    started: Option<DateTime<Local>>,
    finished: Option<DateTime<Local>>,
    updated: DateTime<Local>,
    health: Health,
    description: String,
    jobs: Vec<Job>,
}

impl State {
    pub fn new(manifest: &Manifest) -> Result<Self,String> {
        let now = Local::now();
        let mut state = Self {
            name: format!("{}", now.format("%Y%m%d_%H%M%S")),
            store_path: manifest.store_path.clone(),
            path: manifest.state_path.clone(), //Initialized below
            success_path: None, //Initialized below
            permanent_path: None, //Initialized below
            parent_path: None, //Initialized below
            parent: None, // Initialized below
            started: Some(now.clone()),
            updated: now.clone(),
            finished: None,
            health: Health::Setup,
            description: format!("This backup is in the setup phase. No data has been processed."),
            jobs: manifest.jobs.clone(),
        };
        if let Some(path) = &state.path {
            if let Some(store_path) = &state.store_path {
                state.success_path = Some(store_path.join(Path::new("last_successful_backup.state")));
            }
            state.permanent_path = Some(state.stored_path(path));
        }

        let parent_path = match &manifest.parent_state_path {
            Some(parent_state_path) => {
                Some(parent_state_path.clone())
            },
            None => {
                // Find a previous state by looking at anything already existing at success_path.
                if let Some(success_path) = &state.success_path {
                    if success_path.exists() {
                        Some(success_path.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            },
        };

        state.validate()?;
        if let Some(parent_path) = &parent_path {
            let mut seen_paths = HashSet::new();
            if let Some(path) = &manifest.state_path {
                seen_paths.insert(path.to_path_buf());
            }
            let parent = Self::from_file_recursive(parent_path, seen_paths)?;
            state.parent_path = Some(parent.permanent_path.clone().unwrap());
            state.parent = Some(Box::new(parent));
        }

        if let Some(store_path) = &state.store_path {
            let base_stored_path = store_path.join(Path::new(&state.name));
            if base_stored_path.exists() {
                return Err(format!("A file or directory already exists at path {}", base_stored_path.display()));
            }
            if let Err(e) = std::fs::create_dir(&base_stored_path) {
                return Err(format!("Failed to create backup directory {}: {:?}", base_stored_path.display(), e));
            }
        }

        Ok(state)
    }

    /// Record state to all necessary files
    pub fn commit(&mut self) -> Result<(),String> {
        self.updated = Local::now();
        let mut paths: Vec<&Path> = Vec::new();
        if let Some(path) = &self.path {
            paths.push(path);
        }
        if self.health == Health::Success {
            if let Some(success_path) = &self.success_path {
                paths.push(success_path);
            }
        }
        if let Some(permanent_path) = &self.permanent_path {
            paths.push(permanent_path);
        }
        for path in paths {
            match File::create(path) {
                Ok(file) => {
                    if let Err(e) = serde_yaml::to_writer::<File,State>(file, self) {
                        return Err(format!("Failed to commit state to file {}: {:?}", path.display(), e));
                    }
                },
                Err(e) => {
                    return Err(format!("Failed to open state file {} for commit: {:?}", path.display(), e));
                }
            }
        }
        Ok(())
    }

    /// Load state from file recursively
    pub fn from_file(path: &Path) -> Result<Self,String> {
        Self::from_file_recursive(path, HashSet::new())
    }

    fn from_file_recursive(path: &Path, mut seen_paths: HashSet<PathBuf>) -> Result<Self,String> {
        if let Some(duplicate) = seen_paths.replace(path.to_path_buf()) {
            return Err(format!("State file cyclic dependency detected! Seen {} more than once.", duplicate.display()));
        }

        match File::open(path) {
            Ok(file) => {
                match serde_yaml::from_reader::<File,State>(file) {
                    Ok(mut state) => {
                        if let Some(permanent_path) = &state.permanent_path {
                            if permanent_path != path {
                                // Load file from the official location instead.
                                eprintln!("State file {} is redirecting to its permanent path, {}", path.display(), permanent_path.display());
                                return Self::from_file_recursive(&permanent_path, seen_paths);
                            }
                        } else {
                            return Err(format!("Saved state files must have a permanent path set"));
                        }
                        state.path = Some(path.to_path_buf());
                        state.validate()?;
                        if let Some(parent_path) = &state.parent_path {
                            let parent = Self::from_file_recursive(parent_path, seen_paths)?;
                            state.check_parent(&parent)?;
                            state.parent = Some(Box::new(parent));
                        }
                        Ok(state)
                    },
                    Err(e) => {
                        Err(format!("Failed to read a valid state from file {}: {:?}", path.display(), e))
                    },
                }
            },
            Err(e) => {
                Err(format!("Failed to open state file {}: {:?}", path.display(), e))
            },
        }
    }

    fn sources_to_jobs<'s>(&'s self) -> HashMap<&'s Path, &'s Job> {
        self.jobs
            .iter()
            .map(|job: &Job| {
                (job.source.as_path(), job)
            })
            .collect()
    }

    pub fn source_to_job<'s>(&'s self, source: &Path) -> &'s Job {
        for job in &self.jobs {
            if job.source == source {
                return job;
            }
        }
        panic!("Job with source {} not found", source.display());
    }

    /// Get a vector of all parents (excluding self)
    pub fn history(&self) -> Vec<&State> {
        if let Some(parent) = &self.parent {
            let mut history = parent.history();
            history.push(&parent);
            history
        } else {
            Vec::new()
        }
    }

    pub fn parent(&self) -> Option<&State> {
        match &self.parent {
            Some(boxed_state) => Some(&**boxed_state),
            None => None,
        }
    }

    fn validate(&self) -> Result<(),String> {
        let uniq_sources: HashSet<&Path> = self.jobs
            .iter()
            .map(|job| {
                job.source.as_path()
            })
            .collect();
        if uniq_sources.len() != self.jobs.len() {
            return Err(format!("Backup contains duplicate sources"));
        }
        let uniq_destinations: HashSet<&Path> = self.jobs
            .iter()
            .map(|job| {
                job.storage.destination.as_path()
            })
            .collect();
        if uniq_destinations.len() != self.jobs.len() {
            return Err(format!("Backup contains duplicate destinations"));
        }
        Ok(())
    }

    fn check_parent(&self, parent: &Self) -> Result<(),String> {
        let self_name = self.path.as_ref().unwrap().display();
        let parent_name = parent.path.as_ref().unwrap().display();

        if parent.health != Health::Success {
            return Err(format!("State {} does not represent a successful backup", parent_name));
        }
        let parent_sources_to_jobs: HashMap<&Path, &Job> = parent.sources_to_jobs();
        for self_job in &self.jobs {
            let parent_job = match parent_sources_to_jobs.get(self_job.source.as_path()) {
                Some(parent_job) => parent_job,
                None => {
                    return Err(format!("State {} does not contain source {}", parent_name, self_job.source.display()));
                },
            };
            if parent_job.chunk_size != self_job.chunk_size {
                return Err(format!("State {} has incompatible chunk size {}", parent_name, parent_job.chunk_size));
            }
        }
        if self.jobs != parent.jobs {
            return Err(format!("State files {} and {} are incompatible as their job lists do not match.", self_name, parent_name));
        }
        Ok(())
    }

    pub fn mark_finished(&mut self) {
        self.finished = Some(Local::now());
    }

    /// Update the milestone information in the state and save the state to file.
    pub fn milestone(&mut self, health: Health, description: &str) -> Result<(),String> {
        self.health = health;
        self.description = String::from(description);
        self.commit()?;
        Ok(())
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn stored_path(&self, path: &Path) -> PathBuf {
        if let Some(store_path) = &self.store_path {
            store_path.join(Path::new(&self.name)).join(path)
        } else {
            path.to_path_buf()
        }
    }
}
