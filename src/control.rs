use std::sync::mpsc::{Sender,Receiver,TryRecvError};
use serde::{Serialize,Deserialize};
use crate::job::Job;

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
pub struct Manifest {
    pub jobs: Vec<Job>,
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
