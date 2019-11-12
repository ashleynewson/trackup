use std::ffi::{OsStr,OsString};
use libc::{uid_t,gid_t,mode_t};
use std::path::{Path,PathBuf};
use std::collections::{HashSet,HashMap};
use std::fs::File;
use std::time::Duration;
use serde::{Serialize,Deserialize};


// Just helps label what's option because there's no proper default.
type Required<T> = Option<T>;

pub trait Internalize<O> {
    fn internalize(&self) -> Result<O,String>;
}
impl<O, I: Internalize<O>> Internalize<Vec<O>> for Vec<I> {
    fn internalize(&self) -> Result<Vec<O>,String> {
        let mut transformed = Vec::with_capacity(self.len());
        for part in self {
            let transformed_part: O = part.internalize()?;
            transformed.push(transformed_part);
        }
        Ok(transformed)
    }
}

trait WrappedInternalize<O, W> {
    fn maybe_internalize(&self) -> Result<W,String>;
    fn require_internalize(&self) -> Result<O,String>;
}
impl<O, I: Internalize<O>> WrappedInternalize<O,Option<O>> for Option<I> {
    fn maybe_internalize(&self) -> Result<Option<O>,String> {
        match &self {
            Some(x) => {
                let internalized = x.internalize()?;
                Ok(Some(internalized))
            },
            None => Ok(None),
        }
    }
    fn require_internalize(&self) -> Result<O,String> {
        match &self {
            Some(x) => {
                x.internalize()
            },
            None => Err(format!("Required field omitted")),
        }
    }
}

trait Require<T> {
    fn require(&self) -> Result<T,String>;
}
impl<T: Clone> Require<T> for Required<T> {
    fn require(&self) -> Result<T,String> {
        match self.as_ref() {
            Some(x) => Ok(x.clone()),
            None => Err(format!("Required field omitted")),
        }
    }
}

trait Maybe<I,O> {
    fn maybe<F: Fn(&I)->Result<O,String>>(&self, filter: F) -> Result<Option<O>,String>;
}
impl<I,O> Maybe<I,O> for Option<I> {
    fn maybe<F: Fn(&I)->Result<O,String>>(&self, filter: F) -> Result<Option<O>,String> {
        match self.as_ref() {
            Some(x) => Ok(Some(filter(x)?)),
            None => Ok(None),
        }
    }
}

#[derive(Clone,Serialize,Deserialize)]
pub struct Config {
    pub tracing_path: PathBuf,
    pub sys_path: PathBuf,
    pub trace_buffer_size: usize,
    pub progress_logging: Option<ProgressLogging>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tracing_path: Path::new("/sys/kernel/debug/tracing").to_path_buf(),
            sys_path: Path::new("/sys").to_path_buf(),
            trace_buffer_size: 8192,
            progress_logging: None,
        }
    }
}

impl Internalize<super::Config> for Config {
    fn internalize(&self) -> Result<super::Config,String> {
        let progress_logging = self.progress_logging.maybe_internalize()?;

        Ok(super::Config {
            tracing_path: self.tracing_path.clone(),
            sys_path: self.sys_path.clone(),
            trace_buffer_size: self.trace_buffer_size,
            progress_logging,
        })
    }
}

#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ProgressStyle {
    Plain,
    Color,
}

#[derive(Clone,Serialize,Deserialize)]
pub struct ProgressLogging {
    pub style: ProgressStyle,
    /// Time between progress reports in seconds
    pub interval: f64,
    pub clear_screen: bool,
    pub max_diagram_size: usize,
}

impl Default for ProgressLogging {
    fn default() -> Self {
        Self {
            style: ProgressStyle::Plain,
            interval: 10.0,
            clear_screen: false,
            max_diagram_size: 1024,
        }
    }
}

impl Internalize<super::ProgressLogging> for ProgressLogging {
    fn internalize(&self) -> Result<super::ProgressLogging,String> {
        let diagram_cells =
            match self.style {
                ProgressStyle::Color => {
                    &super::COLOR_DIAGRAM_CELLS
                },
                ProgressStyle::Plain => {
                    &super::PLAIN_DIAGRAM_CELLS
                }
            }
            .iter()
            .map(|x| {String::from(*x)})
            .collect();

        Ok(super::ProgressLogging {
            update_period: duration_from_f64(self.interval)?,
            exclusive: self.clear_screen,
            max_diagram_size: self.max_diagram_size,
            diagram_cells,
            diagram_cells_reset: String::from(match self.style {
                ProgressStyle::Color => "\x1b[m",
                ProgressStyle::Plain => "",
            }),
        })
    }
}

#[derive(Clone,Serialize,Deserialize)]
struct Manifest {
    pub jobs: Vec<Job>,
    pub do_sync: bool,
    pub locking: Option<Locking>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            do_sync: true,
            locking: None,
        }
    }
}

impl Internalize<super::Manifest> for Manifest {
    fn internalize(&self) -> Result<super::Manifest,String> {
        let jobs = self.jobs.internalize()?;
        let locking = self.locking.maybe_internalize()?;
        Ok(super::Manifest {
            jobs,
            do_sync: self.do_sync,
            locking,
        })
    }
}

#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
enum LockBehaviour {
    Existence, // Require that the file not exist at time of test - chance of race conditions
    SharedLock, // Require a shared lock on the file
    ExclusiveLock, // Require an exclusive lock on the file
}

impl Internalize<crate::lock::LockBehaviour> for LockBehaviour {
    fn internalize(&self) -> Result<crate::lock::LockBehaviour,String> {
        Ok(match self {
            LockBehaviour::Existence     => crate::lock::LockBehaviour::Existence,
            LockBehaviour::SharedLock    => crate::lock::LockBehaviour::SharedLock,
            LockBehaviour::ExclusiveLock => crate::lock::LockBehaviour::ExclusiveLock,
        })
    }
}

#[derive(Clone,Serialize,Deserialize)]
struct FileLock {
    pub path: Required<PathBuf>,
    pub behaviour: Required<LockBehaviour>,
    pub create_as_user: Option<String>,
    pub create_as_group: Option<String>,
    pub create_with_mode: Option<String>,
}

impl Default for FileLock {
    fn default() -> Self {
        Self {
            path: None,
            behaviour: None,
            create_as_user: None,
            create_as_group: None,
            create_with_mode: None,
        }
    }
}

impl Internalize<crate::lock::FileLock> for FileLock {
    fn internalize(&self) -> Result<crate::lock::FileLock,String> {
        Ok(crate::lock::FileLock {
            path: self.path.require()?,
            behaviour: self.behaviour.require_internalize()?,
            create_uid: self.create_as_user.maybe(|x| uid_from_str(x))?,
            create_gid: self.create_as_group.maybe(|x| gid_from_str(x))?,
            create_mode: self.create_with_mode.maybe(|x| mode_from_str(x))?,
        })
    }
}

#[derive(Clone,Serialize,Deserialize)]
struct CommandLock {
    pub program: Required<PathBuf>,
    pub args: Vec<OsString>,
    pub user: Option<OsString>, // Run as UID
    pub group: Option<OsString>, // Run as GID
    pub preserve_envs: Option<HashSet<OsString>>, // None means don't clear. Some but empty means preserve nothing.
    pub envs: HashMap<OsString,OsString>,
    pub locking_timeout: Option<f64>,
    pub unlocking_timeout: Option<f64>,
}

impl Default for CommandLock {
    fn default() -> Self {
        Self {
            program: None,
            args: Vec::new(),
            user: None,
            group: None,
            preserve_envs: None,
            envs: HashMap::new(),
            locking_timeout: None,
            unlocking_timeout: None,
        }
    }
}

impl Internalize<crate::lock::CommandLock> for CommandLock {
    fn internalize(&self) -> Result<crate::lock::CommandLock,String> {
        Ok(crate::lock::CommandLock {
            program: self.program.require()?,
            args: self.args.clone(),
            uid: self.user.maybe(|x| uid_from_str(x))?,
            gid: self.group.maybe(|x| uid_from_str(x))?,
            preserve_envs: self.preserve_envs.clone(),
            envs: self.envs.clone(),
            locking_timeout: self.locking_timeout.maybe(|x| duration_from_f64(*x))?,
            unlocking_timeout: self.unlocking_timeout.maybe(|x| duration_from_f64(*x))?,
        })
    }
}

#[derive(Clone,Serialize,Deserialize)]
struct Locking {
    pub command_locks: Vec<CommandLock>,
    pub file_locks: Vec<FileLock>,
    pub time_limit: Option<f64>,
    pub cooldown: Option<f64>,
}

impl Default for Locking {
    fn default() -> Self {
        Self {
            command_locks: Vec::new(),
            file_locks: Vec::new(),
            time_limit: None,
            cooldown: None,
        }
    }
}

impl Internalize<super::Locking> for Locking {
    fn internalize(&self) -> Result<super::Locking,String> {
        Ok(super::Locking {
            command_locks: self.command_locks.internalize()?,
            file_locks: self.file_locks.internalize()?,
            time_limit: self.time_limit.maybe(|x| duration_from_f64(*x))?,
            cooldown: self.cooldown.maybe(|x| duration_from_f64(*x))?,
        })
    }
}

#[derive(Clone,Serialize,Deserialize)]
struct Job {
    pub source: Required<PathBuf>,
    pub destination: Required<PathBuf>,
    pub chunk_size: Required<usize>,
    pub reuse_output: bool,
}

impl Default for Job {
    fn default() -> Self {
        Self {
            source: None,
            destination: None,
            chunk_size: None,
            reuse_output: false,
        }
    }
}

impl Internalize<super::Job> for Job {
    fn internalize(&self) -> Result<super::Job,String> {
        let chunk_size = self.chunk_size.require()?;
        if chunk_size < 512 {
            return Err(format!("chunk_size must be at least 512"));
        }
        if chunk_size % 512 != 0 {
            return Err(format!("chunk_size must be a multiple of 512"));
        }
        Ok(super::Job {
            source: self.source.require()?,
            destination: self.destination.require()?,
            chunk_size,
            reuse_output: self.reuse_output,
        })
    }
}

pub fn read_config_file(path: &Path) -> Result<super::Config,String> {
    match File::open(path) {
        Ok(file) => {
            match serde_yaml::from_reader::<File,Config>(file) {
                Ok(config) => {
                    config.internalize()
                },
                Err(e) => {
                    Err(format!("Failed to read a valid config struture from file {}: {:?}", path.display(), e))
                }
            }
        },
        Err(e) => {
            Err(format!("Failed to open config file {}: {:?}", path.display(), e))
        },
    }
}

pub fn read_manifest_file(path: &Path) -> Result<super::Manifest,String> {
    match File::open(path) {
        Ok(file) => {
            match serde_yaml::from_reader::<File,Manifest>(file) {
                Ok(manifest) => {
                    manifest.internalize()
                },
                Err(e) => {
                    Err(format!("Failed to read a valid manifest struture from file {}: {:?}", path.display(), e))
                }
            }
        },
        Err(e) => {
            Err(format!("Failed to open manifest file {}: {:?}", path.display(), e))
        },
    }
}

fn duration_from_f64(secs: f64) -> Result<Duration, String> {
    if secs <= 0.0 {
        return Err(format!("duration must be positive"));
    }
    // Somewhat arbitrary, but easy to reason about
    if !(secs.is_finite() && secs >= 0.0 && secs <= 1_000_000_000.0) {
        return Err(format!("{} must be a finite float between 0 and 1 billion", secs));
    }
    Ok(Duration::from_secs_f64(secs))
}

fn uid_from_str<T: AsRef<OsStr>>(user_str: T) -> Result<uid_t, String> {
    let user_str = user_str.as_ref();
    if let Some(utf8_str) = user_str.to_str() {
        if let Ok(uid) = utf8_str.parse::<uid_t>() {
            return Ok(uid);
        }
    }
    if let Some(user) = users::get_user_by_name(user_str) {
        return Ok(user.uid());
    }
    return Err(format!("{} is not a valid user name or id", user_str.to_string_lossy()));
}

fn gid_from_str<T: AsRef<OsStr>>(group_str: T) -> Result<gid_t, String> {
    let group_str = group_str.as_ref();
    if let Some(utf8_str) = group_str.to_str() {
        if let Ok(gid) = utf8_str.parse::<gid_t>() {
            return Ok(gid);
        }
    }
    if let Some(group) = users::get_group_by_name(group_str) {
        return Ok(group.gid());
    }
    return Err(format!("{} is not a valid group name or id", group_str.to_string_lossy()));
}

fn mode_from_str<T: AsRef<str>>(mode_str: T) -> Result<mode_t, String> {
    let mode_str = mode_str.as_ref();
    match u32::from_str_radix(mode_str, 8) {
        Ok(mode) => {
            if mode > 0o7777 {
                return Err(format!("{} is too big for a standard octal mode. Only octals within the range 0000 to 7777 are supported.", mode_str));
            }
            Ok(mode)
        },
        Err(e) => {
            Err(format!("{} is not a valid octal mode for permissions: {:?}", mode_str, e))
        }
    }
}
