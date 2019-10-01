use std::thread;
use std::sync::mpsc::{channel,sync_channel};
use std::os::unix::net::{UnixListener};
use std::io::{BufReader,BufWriter};
use std::path::Path;

use crate::control::{Request,Response,Manifest,ManagementInterface,ManagementTicket,Status,LastResult};
use crate::config::Config;


pub fn start_server(socket_path: &Path) -> ManagementInterface {
    let (request_sender, request_receiver) = sync_channel(0);

    let listener = UnixListener::bind(socket_path).unwrap();
    eprintln!("Management server listening on socket: {}", socket_path.display());
    thread::spawn(move || {
        for handler in listener.incoming() {
            match handler {
                Ok(stream) => {
                    let request_sender = request_sender.clone();
                    thread::spawn(move || {
                        let mut request_reader  = BufReader::new(&stream);
                        let mut response_writer = BufWriter::new(&stream);
                        loop {
                            match serde_json::from_reader(&mut request_reader as &mut dyn std::io::Read) {
                                Ok(request) => {
                                    let (response_sender, response_receiver) = channel();
                                    let ticket = ManagementTicket::new(request, response_sender);
                                    request_sender.send(ticket).expect("Could not send management ticket");
                                    let response = response_receiver.recv().expect("Did not receive management response");
                                    serde_json::to_writer(&mut response_writer as &mut dyn std::io::Write, &response).unwrap();
                                },
                                Err(e) => {
                                    if let serde_json::error::Category::Eof = e.classify() {
                                        break;
                                    } else {
                                        panic!("Management socket deserialization error: {:?}", e);
                                    }
                                },
                            }
                        }
                    });
                },
                Err(e) => {
                    panic!("Server socket error {:?}\n", e);
                },
            }
        }
    });

    ManagementInterface::new(Some(request_receiver))
}

pub fn task_loop(config: &Config, management_interface: &ManagementInterface, initial_manifest: Option<Manifest>) {
    let mut last_result: Option<LastResult> = None;

    let run = |manifest| {
        eprintln!("Starting backup");
        let result = crate::copier::run(config, &manifest, management_interface);
        if let Err(e) = result {
            eprintln!("Backup failed: {:?}", e);
        }
        Some(LastResult {
            manifest,
            time: std::time::SystemTime::now(),
            result,
        })
    };

    if let Some(manifest) = initial_manifest {
        last_result = run(manifest);
    }

    loop {
        let ticket = management_interface.get_ticket_blocking();
        match ticket.request.clone() {
            Request::Start(manifest) => {
                ticket.respond(Response::Start(Ok(())));
                last_result = run(manifest.clone());
            },
            Request::Cancel => {
                ticket.respond(Response::Cancel(Err(String::from("There is currently no running backup to cancel"))));
            },
            Request::Pause => {
                ticket.respond(Response::Pause(Err(String::from("There is currently no running backup to pause"))));
            },
            Request::Resume => {
                ticket.respond(Response::Resume(Err(String::from("There is currently no running backup to resume"))));
            },
            Request::Query(_) => {
                let status =
                    match &last_result {
                        Some(last_result) => {
                            Status::Ended(last_result.clone())
                        },
                        None => {
                            Status::Waiting
                        },
                    };
                ticket.respond(Response::Query(status));
            },
        }
    }
}
