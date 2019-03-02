use crate::executor::*;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use uuid::Uuid;

pub type WorkerUuid = Uuid;

pub struct Worker {
    pub uuid: WorkerUuid,
    pub name: String,
    pub sender: Sender<String>,
    pub receiver: Receiver<String>,
}

pub struct WorkerConn {
    pub uuid: WorkerUuid,
    pub name: String,
    pub sender: Sender<String>,
    pub receiver: Receiver<String>,
}

impl Worker {
    pub fn new(name: &str) -> (Worker, WorkerConn) {
        let (tx, rx_worker) = channel();
        let (tx_worker, rx) = channel();
        let uuid = Uuid::new_v4();
        (
            Worker {
                uuid: uuid.clone(),
                name: name.to_owned(),
                sender: tx_worker,
                receiver: rx_worker,
            },
            WorkerConn {
                uuid: uuid,
                name: name.to_owned(),
                sender: tx,
                receiver: rx,
            },
        )
    }

    pub fn work(self) -> Result<(), Error> {
        trace!("Worker {} ready, asking for work", self);
        serialize_into(&WorkerClientMessage::GetWork, &self.sender)?;
        loop {
            let message = deserialize_from::<WorkerServerMessage>(&self.receiver);
            match message {
                Ok(WorkerServerMessage::Work(what)) => {
                    info!("Worker {} got job: {}", self, what);
                    thread::sleep(std::time::Duration::from_secs(1));
                    serialize_into(
                        &WorkerClientMessage::WorkerDone((
                            true,
                            format!("Just completed: '{}'", what),
                        )),
                        &self.sender,
                    )?;
                    serialize_into(&WorkerClientMessage::GetWork, &self.sender)?;
                }
                Err(e) => {
                    let cause = e.find_root_cause().to_string();
                    error!("Connection error: {}", cause);
                    if cause == "receiving on a closed channel" {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for WorkerConn {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "'{}' ({})", self.name, self.uuid)
    }
}

impl std::fmt::Display for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "'{}' ({})", self.name, self.uuid)
    }
}