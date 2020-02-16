use ::serde::{Serialize,Deserialize};
use std::collections::HashSet;
use std::fs::File;
use std::path::{Path,PathBuf};

/// Records a list of states in a store path
#[derive(Clone,Serialize,Deserialize)]
pub struct StoreState {
    #[serde(skip)]
    path: PathBuf,
    pub current: Option<String>,
    pub states: HashSet<String>,
}

// #[derive(Clone,Serialize,Deserialize)]
// pub struct StateRecord {
//     name: String,
// }

impl StoreState {
    pub fn open_dir(path: &Path) -> Result<StoreState,String> {
        StoreState::open(&path.join(Path::new("store.yaml")))
    }
    pub fn open(path: &Path) -> Result<StoreState,String> {
        match File::open(path) {
            Ok(file) => {
                match serde_yaml::from_reader::<File,StoreState>(file) {
                    Ok(mut state) => {
                        state.path = path.to_path_buf();
                        state.validate();
                        Ok(state)
                    }
                    Err(e) => {
                        Err(format!("Failed to read a valid store state from file {}: {:?}", path.display(), e))
                    },
                }
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        eprintln!("Warning: no store state file found - assuming first run.");
                        Ok(Self {
                            path: path.to_path_buf(),
                            current: None,
                            states: HashSet::new(),
                        })
                    },
                    _ => {
                        Err(format!("Failed to open store state file {}: {:?}", path.display(), e))
                    },
                }
            },
        }
    }
    pub fn path(&self) -> &Path {
        self.path.as_ref()
    }

    pub fn validate(&self) {
        if let Some(current) = &self.current {
            if !self.states.contains(current) {
                panic!("Store's current state is set to unknown state");
            }
        }
    }

    pub fn commit(&self) -> Result<(),String> {
        let path = self.path();
        let temp_path = path.with_extension("new");

        self.validate();

        match File::create(&temp_path) {
            Ok(file) => {
                if let Err(e) = serde_yaml::to_writer::<&File,StoreState>(&file, self) {
                    return Err(format!("Failed to commit outbound store state to file {}: {:?}", temp_path.display(), e));
                }
                if let Err(e) = file.sync_all() {
                    return Err(format!("Failed to sync outbound store state file {}: {:?}", temp_path.display(), e));
                }
            },
            Err(e) => {
                return Err(format!("Failed to open outbound store state file {} for commit: {:?}", temp_path.display(), e));
            }
        }
        if let Err(e) = std::fs::rename(&temp_path, path) {
            return Err(format!("Failed to move new store state {} to {}: {:?}", temp_path.display(), path.display(), e));
        }
        Ok(())
    }
}
