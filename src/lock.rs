// Warning: This module permits arbitrary code execution. However, it is
// believed that this is acceptable as an attacker must already have root
// access to utilise this software. Note also that as this software
// essentially copies files/devices, this is already as powerful as arbitrary
// code execution. Users of the software should still exercise suitable
// precautions for any hooked scripts or binaries.

use std::collections::{HashMap,HashSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child,Command,Stdio};
use std::fs::{File,OpenOptions};
use std::time::{Duration,Instant};
use std::sync::{Arc,Mutex};
use libc::{uid_t,gid_t,mode_t};
use nix::fcntl::{FlockArg,FcntlArg};
use nix::unistd::{Uid,Gid,Pid,ForkResult};
use nix::sys::wait::WaitStatus;
use serde::{Serialize,Deserialize};
use crate::control::{Config,Manifest};
use crate::quick_io::{assert_read, poll_read};



pub trait CommitmentCore<'b> {}
pub struct Commitment<'b> {
    _core: Box<dyn CommitmentCore<'b> + 'b>,
}
impl<'b> Commitment<'b> {
    fn new<T: CommitmentCore<'b> + 'b>(core: T) -> Commitment<'b> {
        Commitment {
            _core: Box::new(core)
        }
    }
}

pub trait Lock {
    fn lock<'b>(&'b self) -> Result<Commitment<'b>,()>;
}



#[derive(Clone,Serialize,Deserialize)]
pub enum LockBehaviour {
    Existence, // Require that the file not exist at time of test - chance of race conditions
    SharedLock, // Require a shared lock on the file
    ExclusiveLock, // Require an exclusive lock on the file
}

/// A file which should be absent before backups can be committed.
#[derive(Clone,Serialize,Deserialize)]
pub struct FileLock {
    path: PathBuf,
    behaviour: LockBehaviour,
    create_uid: Option<uid_t>,
    create_gid: Option<gid_t>,
    create_mode: Option<mode_t>,
}


struct FileCommitment<'b> {
    _lock: &'b FileLock,
    _file: Option<File>,
}
impl<'b> FileCommitment<'b> {
    pub(self) fn new(lock: &'b FileLock, file: Option<File>) -> FileCommitment<'b> {
        Self {
            _lock: lock,
            _file: file,
        }
    }
}
impl<'b> CommitmentCore<'b> for FileCommitment<'b> {}

impl FileLock {
    /// Open the lock file. If it does not exist, first create it with
    /// any specified permissions. If it does exist, don't change
    /// anything
    fn open_file(&self) -> Result<File,()> {
        // This is not an optimisation - we absolutely do not want to
        // either replace an existing file or modify its ownership or
        // permissions.
        if !self.path.exists() {
            eprintln!("Warning: attempting to create lock file as it doesn't exist");
            match nix::unistd::fork() {
                Ok(ForkResult::Parent{child, ..}) => {
                    match nix::sys::wait::waitpid(child, None) {
                        Ok(WaitStatus::Exited(_, code)) => {
                            if code == 0 {
                                eprintln!("Created lock file {}", self.path.display());
                            } else {
                                eprintln!("Wait on lock file creation child returned failure code {}", code);
                                return Err(());
                            }
                        },
                        Ok(o) => {
                            panic!("Unexpected waitpid result: {:?}", o);
                        },
                        Err(e) => {
                            panic!("Wait on child failed: {:?}", e);
                        }
                    }
                },
                Ok(ForkResult::Child) => {
                    if let Some(gid) = self.create_gid {
                        nix::unistd::setgid(Gid::from_raw(gid)).unwrap();
                    }
                    if let Some(uid) = self.create_uid {
                        nix::unistd::setuid(Uid::from_raw(uid)).unwrap();
                    }
                    let file = match OpenOptions::new().read(true).write(true).create_new(true).open(&self.path) {
                        Ok(file) => {
                            file
                        },
                        Err(e) => {
                            eprintln!("Create file failed for {}: {:?}", self.path.display(), e);
                            std::process::exit(1);
                        },
                    };
                    if let Some(mode) = self.create_mode {
                        unsafe {
                            if libc::fchmod(file.as_raw_fd(), mode) != 0 {
                                eprintln!("Chmod failed for {}", self.path.display());
                                std::process::exit(1);
                            }
                        }
                    }
                    std::process::exit(0);
                },
                Err(_) => {
                    panic!("Unable to fork");
                }
            }
        }
        match OpenOptions::new().read(true).open(&self.path) {
            Ok(file) => {
                Ok(file)
            },
            Err(_) => {
                println!("Could not open lock file {} (at least read permissions needed)", self.path.display());
                Err(())
            },
        }
    }
}
impl Lock for FileLock {
    fn lock<'b>(&'b self) -> Result<Commitment<'b>,()> {
        match &self.behaviour {
            LockBehaviour::Existence => {
                if !self.path.exists() {
                    Ok(Commitment::new(FileCommitment::new(self, None)))
                } else {
                    Err(())
                }
            },
            behaviour => {
                let file = self.open_file()?;
                let flock_arg = match behaviour {
                    LockBehaviour::Existence     => panic!("Unreachable"),
                    LockBehaviour::SharedLock    => FlockArg::LockSharedNonblock,
                    LockBehaviour::ExclusiveLock => FlockArg::LockExclusiveNonblock,
                };
                if let Ok(()) = nix::fcntl::flock(file.as_raw_fd(), flock_arg) {
                    Ok(Commitment::new(FileCommitment::new(self, Some(file))))
                } else {
                    Err(())
                }
            },
        }
    }
}



pub struct CommandCommitment<'b> {
    lock: &'b CommandLock,
    child: Child,
}
impl<'b> CommandCommitment<'b> {
    pub(self) fn new(lock: &'b CommandLock, child: Child) -> Self {
        Self {
            lock,
            child,
        }
    }
}
impl<'b> CommitmentCore<'b> for CommandCommitment<'b> {
}
impl<'b> Drop for CommandCommitment<'b> {
    fn drop(&mut self) {
        self.lock.unlock(&mut self.child);
    }
}

/// A command to run as a check before a backup is committed
#[derive(Clone,Serialize,Deserialize)]
pub struct CommandLock {
    program: PathBuf,
    args: Vec<OsString>,
    uid: Option<uid_t>, // Run as UID
    gid: Option<gid_t>, // Run as GID
    preserve_envs: Option<HashSet<OsString>>, // None means don't clear. Some but empty means preserve nothing.
    envs: HashMap<OsString,OsString>,
    locking_timeout: Option<Duration>,
    unlocking_timeout: Option<Duration>,
}

impl Lock for CommandLock {
    fn lock<'b> (&'b self) -> Result<Commitment<'b>,()> {
        let mut command = Command::new(self.program.as_os_str());
        command
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped());

        if let Some(preserve_envs) = self.preserve_envs.as_ref() {
            command.env_clear();
            command.envs(
                std::env::vars_os().filter(
                    |(k, _)| {preserve_envs.contains(k)}
                )
            );
        }
        command.envs(&self.envs);
        if let Some(uid) = self.uid {
            command.uid(uid);
        }
        if let Some(gid) = self.gid {
            command.gid(gid);
        }

        match command.spawn() {
            Ok(mut child) => {
                if let Some(timeout) = self.locking_timeout {
                    let stdout_fd = child.stdout.as_ref().unwrap().as_raw_fd();
                    let prev_flags = nix::fcntl::OFlag::from_bits(nix::fcntl::fcntl(stdout_fd, FcntlArg::F_GETFL).unwrap()).unwrap();
                    let new_flags = prev_flags | nix::fcntl::OFlag::O_NONBLOCK;
                    // Note: this will now be locked for the life of the child
                    nix::fcntl::fcntl(stdout_fd,  FcntlArg::F_SETFL(new_flags)).unwrap();
                    if !poll_read(child.stdout.as_ref().unwrap(), timeout) {
                        eprintln!("Command process locking timed out!");
                        self.unlock(&mut child);
                        return Err(());
                    }
                }
                if let Err(_) = assert_read(child.stdout.as_mut().unwrap(), b"locked\n") {
                    eprintln!("Command process stated something other than 'locked'. Killing.");
                    child.kill().unwrap();
                    return Err(());
                }
                Ok(Commitment::new(CommandCommitment::new(self, child)))
            },
            Err(e) => {
                panic!("Could not spawn command lock process: {:?}", e);
            },
        }
    }
}

impl CommandLock {
    fn unlock(&self, child: &mut Child) {
        let pid = Pid::from_raw(child.id() as i32);
        nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).unwrap();
        if let Some(timeout) = self.unlocking_timeout {
            if !poll_read(child.stdout.as_ref().unwrap(), timeout) {
                eprintln!("Command process unlocking timed out!");
                child.kill().unwrap();
            }
        }
        if let Err(_) = assert_read(child.stdout.as_mut().unwrap(), b"unlocked\n") {
            eprintln!("Command process stated something other than 'unlocked'. Killing.");
            child.kill().unwrap();
        }
        // This isn't very clean right now:
        let mut ended = false;
        for _ in 0..100 {
            match child.try_wait().unwrap() {
                Some(_) => {
                    ended = true;
                    break;
                },
                None => {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
        if !ended {
            eprintln!("Command process has not terminated in one second after unlock. Killing.");
            child.kill().unwrap();
        }
        if !child.wait().unwrap().success() {
            eprintln!("Command process did not exit with success");
        }
    }
}



#[derive(Copy,Clone,Debug,PartialEq)]
pub enum AutoLockerStatus {
    Unlocked,
    Locking,
    Locked,
    Unlocking,
    Cooldown,
}

struct AutoLockerShared {
    pub status: Mutex<AutoLockerStatus>,
    pub joining: Mutex<bool>,
}
pub struct AutoLocker {
    shared: Arc<AutoLockerShared>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl AutoLocker {
    pub fn new(config: &Config, manifest: &Manifest) -> Self {
        let shared = Arc::new(AutoLockerShared {
            status: Mutex::new(AutoLockerStatus::Unlocked),
            joining: Mutex::new(false),
        });
        let join_handle = {
            let shared = Arc::clone(&shared);
            let config = config.clone();
            let manifest = manifest.clone();
            Some(
                std::thread::Builder::new()
                    .name("lock".to_string())
                    .spawn(|| {AutoLocker::run(config, manifest, shared)})
                    .unwrap()
            )
        };
        Self {
            shared,
            join_handle,
        }
    }
    pub fn check(&self) -> AutoLockerStatus {
        if Arc::strong_count(&self.shared) < 2 {
            // This Arc also acts as a canary in case the other thread dies.
            panic!("auto locker thread appears to have died");
        }
        let mut status = self.shared.status.lock().unwrap();
        match *status {
            AutoLockerStatus::Unlocked => {
                *status = AutoLockerStatus::Locking;
                self.join_handle.as_ref().unwrap().thread().unpark();
                AutoLockerStatus::Locking
            },
            status => {
                status
            },
        }
    }

    fn run(_config: Config, manifest: Manifest, shared: Arc<AutoLockerShared>) {
        let mut locks: Vec<&dyn Lock> = Vec::new();
        for lock in &manifest.command_locks {
            locks.push(lock);
        }
        for lock in &manifest.file_locks {
            locks.push(lock);
        }
        let locks = locks; // drop mut

        if locks.len() == 0 {
            // Special case: always act locked if we have no locks
            *shared.status.lock().unwrap() = AutoLockerStatus::Locked;
            while !*shared.joining.lock().unwrap() {
                std::thread::park();
            }
            return;
        }

        let interruptible_timeout = |duration| {
            let start = Instant::now();
            while
                !*shared.joining.lock().unwrap()
                && start.elapsed() < duration
            {
                if let Some(time_left) = duration.checked_sub(start.elapsed()) {
                    std::thread::park_timeout(time_left);
                } else {
                    break;
                }
            }
        };

        while !*shared.joining.lock().unwrap() {
            let status = *shared.status.lock().unwrap();
            match status {
                AutoLockerStatus::Unlocked => {
                    std::thread::park();
                },
                AutoLockerStatus::Locking => {
                    eprintln!("Applying consistency locks...");
                    // Todo: make threaded?
                    (|| {
                        let mut commitments: Vec<Commitment> = Vec::new();
                        for lock in &locks {
                            match lock.lock() {
                                Ok(commitment) => {
                                    commitments.push(commitment);
                                },
                                Err(_) => {
                                    eprintln!("Cannot lock right now. Backing off.");
                                    return;
                                }
                            }
                        }
                        *shared.status.lock().unwrap() = AutoLockerStatus::Locked;
                        eprintln!("Locks acquired.");
                        interruptible_timeout(manifest.lock_time_limit);
                        *shared.status.lock().unwrap() = AutoLockerStatus::Unlocking;
                        eprintln!("Unlocking...");
                    })();
                    *shared.status.lock().unwrap() = AutoLockerStatus::Cooldown;
                    eprintln!("Consistency lock cooldown started...");
                    interruptible_timeout(manifest.lock_cooldown);
                    *shared.status.lock().unwrap() = AutoLockerStatus::Unlocked;
                    eprintln!("Consistency lock cooldown expired.");
                },
                other => {
                    // Unreachable
                    panic!("auto locker is in an unexpected state: {:?}", other);
                },
            }
        }
    }
}
impl Drop for AutoLocker {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            *self.shared.joining.lock().unwrap() = true;
            let join_handle = self.join_handle.take().unwrap();
            join_handle.thread().unpark();
            join_handle.join().unwrap();
        }
    }
}
