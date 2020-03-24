use std::collections::HashMap;
use std::fs::Permissions;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

use failure::{format_err, Error, Fail};
use tempdir::TempDir;
use uuid::Uuid;

use task_maker_dag::*;
use task_maker_store::*;

use crate::executor::WorkerJob;
use crate::proto::*;
use crate::sandbox::{Sandbox, SandboxResult};
use crate::sandbox_runner::SandboxRunner;
use crate::{new_local_channel, ChannelReceiver, ChannelSender};

/// The information about the current job the worker is doing.
struct WorkerCurrentJob {
    /// Job currently waiting for, when there is a job running this should be `None`
    current_job: Option<(Box<WorkerJob>, HashMap<FileUuid, FileStoreHandle>)>,
    /// The currently running sandbox.
    current_sandboxes: Option<Vec<Sandbox>>,
    /// The dependencies that are missing and required for the execution start.
    missing_deps: HashMap<FileStoreKey, Vec<FileUuid>>,
    /// Send to the sandbox_manager the list of files the server is missing.
    server_asked_files: Option<Sender<Vec<FileUuid>>>,
}

/// The worker is the component that receives the work from the server and sends the results back.
/// It computes the results by executing a process inside a sandbox, limiting the available
/// resources and measuring the used ones.
pub struct Worker {
    /// The identifier of this worker.
    uuid: WorkerUuid,
    /// The name of this worker.
    name: String,
    /// The channel that sends messages to the server.
    sender: ChannelSender<WorkerClientMessage>,
    /// The channel that receives messages from the server.
    receiver: ChannelReceiver<WorkerServerMessage>,
    /// A reference to the [`FileStore`](../task_maker_store/struct.FileStore.html).
    file_store: Arc<FileStore>,
    /// Job the worker is currently working on.
    current_job: Arc<Mutex<WorkerCurrentJob>>,
    /// Where to put the sandboxes.
    sandbox_path: PathBuf,
    /// The function that spawns an actual sandbox.
    sandbox_runner: Arc<dyn SandboxRunner>,
}

/// An handle of the connection to the worker.
pub struct WorkerConn {
    /// The identifier of the worker.
    pub uuid: WorkerUuid,
    /// The name of the worker.
    pub name: String,
    /// The channel that sends messages to the worker.
    pub sender: ChannelSender<WorkerServerMessage>,
    /// The channel that receives messages from the server.
    pub receiver: ChannelReceiver<WorkerClientMessage>,
}

/// An error generated by the worker.
#[derive(Debug, Fail)]
enum WorkerError {
    /// A dependency key is missing from the list of file dependencies.
    #[fail(display = "missing key for dependency {}", uuid)]
    MissingDependencyKey { uuid: Uuid },
}

impl WorkerCurrentJob {
    /// Make a new [`WorkerCurrentJob`](struct.WorkerCurrentJob.html).
    fn new() -> WorkerCurrentJob {
        WorkerCurrentJob {
            current_job: None,
            current_sandboxes: None,
            missing_deps: HashMap::new(),
            server_asked_files: None,
        }
    }
}

impl Worker {
    /// Make a new worker attached to a [`FileStore`](../task_maker_store/struct.FileStore.html),
    /// will return a pair with the actual `Worker` and an handle with the channels to connect to
    /// communicate with the worker.
    pub fn new<S: Into<String>, P: Into<PathBuf>, R>(
        name: S,
        file_store: Arc<FileStore>,
        sandbox_path: P,
        runner: R,
    ) -> (Worker, WorkerConn)
    where
        R: SandboxRunner + 'static,
    {
        let (tx, rx_worker) = new_local_channel();
        let (tx_worker, rx) = new_local_channel();
        let uuid = Uuid::new_v4();
        let name = name.into();
        (
            Worker::new_with_channel(
                name.clone(),
                file_store,
                sandbox_path.into(),
                tx_worker,
                rx_worker,
                runner,
            ),
            WorkerConn {
                uuid,
                name,
                sender: tx,
                receiver: rx,
            },
        )
    }

    /// Make a new worker with an already connected channel.
    pub fn new_with_channel<S: Into<String>, P: Into<PathBuf>, R>(
        name: S,
        file_store: Arc<FileStore>,
        sandbox_path: P,
        sender: ChannelSender<WorkerClientMessage>,
        receiver: ChannelReceiver<WorkerServerMessage>,
        runner: R,
    ) -> Worker
    where
        R: SandboxRunner + 'static,
    {
        let uuid = Uuid::new_v4();
        let name = name.into();
        let sandbox_path = sandbox_path.into();
        Worker {
            uuid,
            name,
            sender,
            receiver,
            file_store,
            current_job: Arc::new(Mutex::new(WorkerCurrentJob::new())),
            sandbox_path,
            sandbox_runner: Arc::new(runner),
        }
    }

    /// The worker body, this function will block until the worker disconnects.
    #[allow(clippy::cognitive_complexity)]
    pub fn work(self) -> Result<(), Error> {
        trace!("Worker {} ready, asking for work", self);
        self.sender.send(WorkerClientMessage::GetWork)?;

        // the join handle of the currently running sandbox, if any.
        let mut current_sandbox_thread: Option<JoinHandle<()>> = None;
        macro_rules! start_job {
            ($self:expr, $current_sandbox_thread:expr) => {{
                let thread = execute_job(
                    $self.current_job.clone(),
                    &$self.sender,
                    &$self.sandbox_path,
                    $self.sandbox_runner.clone(),
                )?;
                $current_sandbox_thread = Some(thread);
            }};
        }
        macro_rules! wait_sandbox {
            ($current_sandbox_thread:expr) => {
                if let Some(join_handle) = $current_sandbox_thread.take() {
                    join_handle
                        .join()
                        .map_err(|e| format_err!("Sandbox thread failed: {:?}", e))?;
                }
            };
        }

        loop {
            match self.receiver.recv() {
                Ok(WorkerServerMessage::Work(job)) => {
                    trace!("Worker {} got job: {:?}", self, job);
                    assert!(self.current_job.lock().unwrap().current_job.is_none());
                    wait_sandbox!(current_sandbox_thread);
                    let mut missing_deps: HashMap<FileStoreKey, Vec<FileUuid>> = HashMap::new();
                    let mut handles = HashMap::new();
                    for exec in &job.group.executions {
                        for input in exec.dependencies().iter() {
                            let key = job
                                .dep_keys
                                .get(&input)
                                .ok_or(WorkerError::MissingDependencyKey { uuid: *input })?;
                            match self.file_store.get(&key) {
                                None => {
                                    // ask the file only once
                                    if !missing_deps.contains_key(key) {
                                        self.sender
                                            .send(WorkerClientMessage::AskFile(key.clone()))?;
                                    }
                                    missing_deps.entry(key.clone()).or_default().push(*input);
                                }
                                Some(handle) => {
                                    handles.insert(*input, handle);
                                }
                            }
                        }
                    }
                    let job_ready = missing_deps.is_empty();
                    {
                        let mut current_job = self.current_job.lock().unwrap();
                        current_job.missing_deps = missing_deps;
                        current_job.current_job = Some((job, handles));
                    }
                    if job_ready {
                        start_job!(self, current_sandbox_thread);
                    }
                }
                Ok(WorkerServerMessage::ProvideFile(key)) => {
                    info!("Server sent file {:?}", key);
                    let reader = ChannelFileIterator::new(&self.receiver);
                    let handle = self.file_store.store(&key, reader)?;
                    let should_start = {
                        let mut job = self.current_job.lock().unwrap();
                        let uuids = job
                            .missing_deps
                            .remove(&key)
                            .expect("Server sent a not required dependency");
                        for uuid in uuids {
                            job.current_job
                                .as_mut()
                                .expect("Received file while doing nothing")
                                .1
                                .insert(uuid, handle.clone());
                        }
                        job.missing_deps.is_empty()
                    };
                    if should_start {
                        start_job!(self, current_sandbox_thread);
                    }
                }
                Ok(WorkerServerMessage::Exit) => {
                    info!("Worker {} ({}) is asked to exit", self.name, self.uuid);
                    break;
                }
                Ok(WorkerServerMessage::KillJob(job)) => {
                    let current_job = self.current_job.lock().unwrap();
                    if let Some((worker_job, _)) = current_job.current_job.as_ref() {
                        // check that the job is the same
                        if worker_job.group.uuid == job {
                            if let Some(sandboxes) = current_job.current_sandboxes.as_ref() {
                                // ask the sandbox to kill the process
                                for sandbox in sandboxes {
                                    sandbox.kill();
                                }
                            }
                        }
                    }
                }
                Ok(WorkerServerMessage::AskFiles(files)) => {
                    let mut current_job = self.current_job.lock().unwrap();
                    if let Some(sender) = current_job.server_asked_files.take() {
                        if let Err(e) = sender.send(files) {
                            error!("Cannot send the list of files from the server to the worker manager: {:?}", e);
                        }
                    } else {
                        error!("Unexpected WorkerServerMessage::AskFiles");
                    }
                }
                Err(e) => {
                    let cause = e.find_root_cause().to_string();
                    if cause == "receiving on an empty and disconnected channel" {
                        trace!("Connection closed: {}", cause);
                    } else {
                        error!("Connection error: {}", cause);
                    }
                    if let Some(sandboxes) =
                        self.current_job.lock().unwrap().current_sandboxes.as_ref()
                    {
                        for sandbox in sandboxes {
                            sandbox.kill();
                        }
                    }
                    break;
                }
            }
        }
        {
            // make sure that the worker doesn't wait for the server's response
            let mut current_job = self.current_job.lock().unwrap();
            current_job.server_asked_files.take();
        }
        wait_sandbox!(current_sandbox_thread);
        Ok(())
    }
}

/// Spawn a new thread that will start the sandbox and will send the results back to the server.
fn execute_job(
    current_job: Arc<Mutex<WorkerCurrentJob>>,
    sender: &ChannelSender<WorkerClientMessage>,
    sandbox_path: &Path,
    runner: Arc<dyn SandboxRunner>,
) -> Result<JoinHandle<()>, Error> {
    let (job, sandboxes, fifo_dir, server_asked_files) = {
        let mut current_job = current_job.lock().unwrap();
        let job = current_job
            .current_job
            .as_ref()
            .expect("Worker job is gone");
        let mut boxes = Vec::new();
        let group = &job.0.group;
        let fifo_dir = if group.fifo.is_empty() {
            None
        } else {
            let fifo_dir = TempDir::new_in(sandbox_path, "pipes")?;
            for fifo in &group.fifo {
                let path = fifo_dir
                    .path()
                    .join(fifo.sandbox_path().file_name().unwrap());
                nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRWXU)?;
            }
            Some(fifo_dir)
        };
        let keep_sandboxes = group.config().keep_sandboxes;
        for exec in &group.executions {
            let mut sandbox = Sandbox::new(
                sandbox_path,
                exec,
                &job.1,
                fifo_dir.as_ref().map(|d| d.path().to_owned()),
            )?;
            if keep_sandboxes {
                sandbox.keep();
            }
            boxes.push(sandbox);
        }
        let job = job.0.clone();
        current_job.current_sandboxes = Some(boxes.clone());
        let (sender, receiver) = channel();
        current_job.server_asked_files = Some(sender);
        (job, boxes, fifo_dir, receiver)
    };
    let sender = sender.clone();
    let join_handle = std::thread::Builder::new()
        .name(format!(
            "Sandbox group manager for {}",
            job.group.description
        ))
        .spawn(move || {
            sandbox_group_manager(
                current_job,
                *job,
                sender,
                server_asked_files,
                sandboxes,
                runner,
                fifo_dir,
            )
        })?;
    Ok(join_handle)
}

/// The sandbox group manager spawns the threads of the sandbox of all the executions in the group.
/// Then waits for their outcome and eventually stops the sandboxes if a process fails. When all the
/// sandboxes complete, this manager collects their results and send them back to the server.
///
/// Note that this function owns `fifo_dir`, the `TempDir` where the FIFOs are stored, it has not to
/// be dropped before all the sandboxes end.
fn sandbox_group_manager(
    current_job: Arc<Mutex<WorkerCurrentJob>>,
    job: WorkerJob,
    sender: ChannelSender<WorkerClientMessage>,
    server_asked_files_receiver: Receiver<Vec<FileUuid>>,
    mut sandboxes: Vec<Sandbox>,
    runner: Arc<dyn SandboxRunner>,
    fifo_dir: Option<TempDir>,
) {
    assert_eq!(sandboxes.len(), job.group.executions.len());
    let mut results = vec![None; job.group.executions.len()];
    let mut outputs = HashMap::new();
    let mut output_paths = HashMap::new();

    // in case of simple executions there's no need to spawn the sandbox in a different thread and
    // then join from here
    if job.group.executions.len() == 1 {
        let sandbox = sandboxes.pop().unwrap();
        let result = match sandbox.run(runner.as_ref()) {
            Ok(res) => res,
            Err(e) => SandboxResult::Failed {
                error: e.to_string(),
            },
        };
        let exec = &job.group.executions[0];
        let result = compute_execution_result(exec, result, &sandbox)
            .expect("Cannot compute execution result");
        get_result_outputs(exec, &sandbox, &mut outputs, &mut output_paths);

        results[0] = Some(result);
    // this is the complex case: more than an execution (therefore more than a sandbox)
    // All the sandboxes will run in a separate thread and this thread will wait all of them. When
    // a sandbox is done, it signals to this thread the completion which simply computes the result.
    // When all of them have finished this thread sends the result and exits.
    } else {
        let mut missing = job.group.executions.len();
        let mut handles = Vec::new();
        let (group_sender, receiver) = channel();
        for (index, sandbox) in sandboxes.clone().into_iter().enumerate() {
            handles.push(
                spawn_sandbox(
                    &job.group.description,
                    sandbox,
                    runner.clone(),
                    index,
                    group_sender.clone(),
                )
                .unwrap(),
            );
        }

        while missing > 0 {
            match receiver.recv() {
                Ok((index, result)) => {
                    assert!(results[index].is_none());

                    let exec = &job.group.executions[index];
                    let sandbox = &sandboxes[index];

                    let result = compute_execution_result(exec, result, &sandbox)
                        .expect("Cannot compute execution result");
                    // if the process didn't exit successfully, kill the remaining sandboxes
                    if !result.status.is_success() {
                        for (i, (res, sandbox)) in results.iter().zip(sandboxes.iter()).enumerate()
                        {
                            // do not kill the current process
                            if i != index && res.is_none() {
                                sandbox.kill();
                            }
                        }
                    }

                    get_result_outputs(exec, sandbox, &mut outputs, &mut output_paths);

                    results[index] = Some(result);
                    missing -= 1;
                }
                _ => panic!("The sandboxes didn't exit well"),
            }
        }
        for handle in handles {
            handle.join().expect("Sandbox thread failed");
        }
    }
    // tell the server the results and the list of produced files
    sender
        .send(WorkerClientMessage::WorkerDone(
            results.into_iter().map(Option::unwrap).collect(),
            outputs.clone(),
        ))
        .unwrap();
    // wait for the list of files to send
    match server_asked_files_receiver.recv() {
        Ok(missing_files) => {
            for uuid in missing_files {
                if let Some(key) = outputs.get(&uuid) {
                    sender
                        .send(WorkerClientMessage::ProvideFile(uuid, key.clone()))
                        .unwrap();
                    ChannelFileSender::send(&output_paths[&uuid], &sender).unwrap();
                } else {
                    error!(
                        "Server asked for file {}, which is not known to the worker",
                        uuid
                    );
                }
            }
        }
        Err(e) => {
            // not receiving the list from the server means that the server is going down and does
            // not bother of responding, letting the worker crash will crash the local executor.
            // So just cleanup and exit without asking for more jobs.
            warn!(
                "List of missing files not received from the server: {:?}",
                e
            );
            let mut job = current_job.lock().unwrap();
            job.current_job = None;
            job.current_sandboxes = None;
            return;
        }
    }
    // this job is completed, reset the worker and ask for more work
    let mut job = current_job.lock().unwrap();
    job.current_job = None;
    job.current_sandboxes = None;
    let _ = sender.send(WorkerClientMessage::GetWork);
    // The sandbox may chmod -r the directory, revert it to allow deletion on drop
    if let Some(fifo_dir) = fifo_dir {
        let _ = std::fs::set_permissions(fifo_dir.path(), Permissions::from_mode(0o755));
    }
}

/// Spawn the sandbox of an execution in a different thread and send to the group manager the
/// results.
fn spawn_sandbox(
    description: &str,
    sandbox: Sandbox,
    runner: Arc<dyn SandboxRunner>,
    index: usize,
    group_sender: Sender<(usize, SandboxResult)>,
) -> Result<JoinHandle<()>, Error> {
    Ok(thread::Builder::new()
        .name(format!("Sandbox of {}", description))
        .spawn(move || {
            let res = match sandbox.run(runner.as_ref()) {
                Ok(res) => res,
                Err(e) => SandboxResult::Failed {
                    error: e.to_string(),
                },
            };
            group_sender.send((index, res)).unwrap();
        })?)
}

/// Compute the [`ExecutionResult`](../task_maker_dag/struct.ExecutionResult.html) based on the
/// result of the sandbox.
fn compute_execution_result(
    execution: &Execution,
    result: SandboxResult,
    sandbox: &Sandbox,
) -> Result<ExecutionResult, Error> {
    match result {
        SandboxResult::Success {
            exit_status,
            signal,
            resources,
            was_killed,
        } => Ok(ExecutionResult {
            status: execution.status(exit_status, signal, &resources),
            resources,
            stdout: capture_stream(&sandbox.stdout_path(), execution.capture_stdout)?,
            was_killed,
            was_cached: false,
            stderr: capture_stream(&sandbox.stderr_path(), execution.capture_stderr)?,
        }),
        SandboxResult::Failed { error } => Ok(ExecutionResult {
            status: ExecutionStatus::InternalError(error),
            resources: ExecutionResourcesUsage::default(),
            stdout: None,
            was_killed: false,
            was_cached: false,
            stderr: None,
        }),
    }
}

/// Extract the output files from the result of the sandbox and store them in the provided HashMaps.
fn get_result_outputs(
    exec: &Execution,
    sandbox: &Sandbox,
    outputs: &mut HashMap<FileUuid, FileStoreKey>,
    output_paths: &mut HashMap<FileUuid, PathBuf>,
) {
    if let Some(stdout) = &exec.stdout {
        let path = sandbox.stdout_path();
        outputs.insert(stdout.uuid, FileStoreKey::from_file(&path).unwrap());
        output_paths.insert(stdout.uuid, path);
    }
    if let Some(stderr) = &exec.stderr {
        let path = sandbox.stderr_path();
        outputs.insert(stderr.uuid, FileStoreKey::from_file(&path).unwrap());
        output_paths.insert(stderr.uuid, path);
    }
    for (path, file) in exec.outputs.iter() {
        let path = sandbox.output_path(path);
        // the sandbox process may want to remove a file, consider missing files as empty
        if path.exists() {
            outputs.insert(file.uuid, FileStoreKey::from_file(&path).unwrap());
            output_paths.insert(file.uuid, path.clone());
        } else {
            // FIXME: /dev/null may not be used
            outputs.insert(file.uuid, FileStoreKey::from_file("/dev/null").unwrap());
            output_paths.insert(file.uuid, "/dev/null".into());
        }
    }
}

/// If `count` is `None` do not read anything, otherwise read at most that number of bytes from the
/// `path`.
fn capture_stream(path: &Path, count: Option<usize>) -> Result<Option<Vec<u8>>, Error> {
    if let Some(count) = count {
        let mut file = std::fs::File::open(path)?;
        let mut result = Vec::new();
        let mut buffer = vec![0; 1024];
        let mut read = 0;
        while read < count {
            let n = file.read(&mut buffer)?;
            // EOF
            if n == 0 {
                break;
            } else {
                result.extend_from_slice(&buffer[0..n]);
                read += n;
            }
        }
        Ok(Some(result))
    } else {
        Ok(None)
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
