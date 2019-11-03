use std::path::PathBuf;
use std::time::Duration;
use std::sync::mpsc::{Sender,Receiver,TryRecvError};
use serde::{Serialize,Deserialize};
use crate::lock::{CommandLock,FileLock};

#[derive(Clone,Serialize,Deserialize)]
pub enum Request {
    Start(Manifest),
    Cancel,
    Pause,
    Resume,
    Query(Query),
}

#[derive(Clone,Serialize,Deserialize)]
pub enum Response {
    Start(Result<(),String>),
    Cancel(Result<(),String>),
    Pause(Result<(),String>),
    Resume(Result<(),String>),
    Query(Status),
}

#[derive(Clone,Serialize,Deserialize)]
pub struct ProgressLogging {
    pub update_period: Duration,
    pub exclusive: bool,
    pub max_diagram_size: usize,
    pub diagram_cells: Vec<String>,
    pub diagram_cells_reset: String,
}

#[derive(Clone,Serialize,Deserialize)]
pub struct Config {
    pub tracing_path: PathBuf,
    pub sys_path: PathBuf,
    pub trace_buffer_size: usize,
    pub progress_logging: Option<ProgressLogging>,
}

pub const PLAIN_DIAGRAM_CELLS: [&str; 4] = ["#", "*", ".", "o"];
pub const COLOR_DIAGRAM_CELLS: [&str; 4] = ["\x1b[42m#", "\x1b[41m*", "\x1b[100m.", "\x1b[44mo"];

#[derive(Clone,Serialize,Deserialize)]
pub struct Job {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub chunk_size: usize,
    pub reuse_output: bool,
}

#[derive(Clone,Serialize,Deserialize)]
pub struct Locking {
    pub command_locks: Vec<CommandLock>,
    pub file_locks: Vec<FileLock>,
    pub time_limit: Duration,
    pub cooldown: Duration,
}

#[derive(Clone,Serialize,Deserialize)]
pub struct Manifest {
    pub jobs: Vec<Job>,
    pub do_sync: bool,
    pub locking: Option<Locking>,
}

#[derive(Clone,Serialize,Deserialize)]
pub struct Query {
    pub max_diagram_size: usize,
}

#[derive(Clone,Serialize,Deserialize)]
pub enum Status {
    Waiting,
    Running(RunStatus),
    Ended(LastResult),
}

#[derive(Clone,Serialize,Deserialize)]
pub struct RunStatus {
    pub manifest: Manifest,
    pub progress: Vec<JobProgress>,
    pub paused: bool,
}

#[derive(Clone,Serialize,Deserialize)]
pub struct LastResult {
    pub manifest: Manifest,
    pub time: std::time::SystemTime,
    pub result: Result<(),()>,
}

#[derive(Clone,Serialize,Deserialize)]
pub struct JobProgress {
    pub job: Job,
    pub chunk_count: usize,
    pub cells: Vec<u8>,
    pub chunks_per_cell: usize,
}

pub struct ManagementTicket {
    pub request: Request,
    response_sender: Sender<Response>,
}

impl ManagementTicket {
    pub fn new(request: Request, response_sender: Sender<Response>) -> Self {
        Self {
            request,
            response_sender,
        }
    }

    // Consumes self (only one response is allowed)
    pub fn respond(self, response: Response) {
        if let Err(e) = self.response_sender.send(response) {
            eprintln!("Error responding to management ticket: {:?}", e);
        }
    }
}

pub struct ManagementInterface {
    ticket_receiver: Option<Receiver<ManagementTicket>>,
}

impl ManagementInterface {
    pub fn new(ticket_receiver: Option<Receiver<ManagementTicket>>) -> Self {
        Self {
            ticket_receiver,
        }
    }

    pub fn get_ticket(&self) -> Option<ManagementTicket> {
        match &self.ticket_receiver {
            Some(ticket_receiver) => {
                match ticket_receiver.try_recv() {
                    Ok(ticket) => {
                        Some(ticket)
                    },
                    Err(TryRecvError::Empty) => {
                        None
                    },
                    Err(e) => {
                        panic!("Management interface channel failure: {:?}", e);
                    },
                }
            },
            None => {
                None
            },
        }
    }

    pub fn get_ticket_blocking(&self) -> ManagementTicket {
        match &self.ticket_receiver {
            Some(ticket_receiver) => {
                match ticket_receiver.recv() {
                    Ok(ticket) => {
                        ticket
                    },
                    Err(e) => {
                        panic!("Management interface channel failure: {:?}", e);
                    },
                }
            },
            None => {
                panic!("Blocking request to get ticket when interface has no ticket provider");
            },
        }
    }
}
