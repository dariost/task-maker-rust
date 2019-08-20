use crate::file::*;
use crate::*;
use boxfnonce::BoxFnOnce;
use failure::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use task_maker_store::*;

/// A wrapper around a File provided by the client, this means that the client
/// knows the FileStoreKey and the path to that file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidedFile {
    /// The file handle.
    pub file: File,
    /// The key in the FileStore.
    pub key: FileStoreKey,
    /// Path to the file in the client.
    pub local_path: PathBuf,
}

/// Serializable part of the execution DAG: everything except the callbacks (which are not
/// serializable).
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionDAGData {
    /// All the files provided by the client.
    pub provided_files: HashMap<FileUuid, ProvidedFile>,
    /// All the executions to run.
    pub executions: HashMap<ExecutionUuid, Execution>,
}

/// A computation DAG, this is not serializable because it contains the callbacks of the client.
#[derive(Debug)]
pub struct ExecutionDAG {
    /// Serializable part of the DAG with all the executions and files.
    pub data: ExecutionDAGData,
    /// Actual callbacks of the executions.
    pub execution_callbacks: HashMap<ExecutionUuid, ExecutionCallbacks>,
    /// Actual callbacks of the files.
    pub file_callbacks: HashMap<FileUuid, FileCallbacks>,
}

impl ExecutionDAG {
    /// Create an empty ExecutionDAG, without files and executions.
    pub fn new() -> ExecutionDAG {
        ExecutionDAG {
            data: ExecutionDAGData {
                provided_files: HashMap::new(),
                executions: HashMap::new(),
            },
            execution_callbacks: HashMap::new(),
            file_callbacks: HashMap::new(),
        }
    }

    /// Provide a file for the computation.
    pub fn provide_file<P: Into<PathBuf>>(&mut self, file: File, path: P) -> Result<(), Error> {
        let path = path.into();
        self.data.provided_files.insert(
            file.uuid,
            ProvidedFile {
                file,
                key: FileStoreKey::from_file(&path)?,
                local_path: path,
            },
        );
        Ok(())
    }

    /// Add an execution to the DAG.
    pub fn add_execution(&mut self, execution: Execution) {
        self.data.executions.insert(execution.uuid, execution);
    }

    /// When `file` is ready it will be written to `path`. The file must be present in the dag
    /// before the evaluation starts.
    pub fn write_file_to<F: Into<FileUuid>, P: Into<PathBuf>>(&mut self, file: F, path: P) {
        self.file_callback(file.into()).write_to = Some(path.into());
    }

    /// Call `callback` with the first `limit` bytes of the file when it's
    /// ready. The file must be present in the DAG before the evaluation
    /// starts.
    pub fn get_file_content<G: Into<FileUuid>, F>(&mut self, file: G, limit: usize, callback: F)
    where
        F: (FnOnce(Vec<u8>) -> ()) + 'static,
    {
        self.file_callback(file.into()).get_content = Some((limit, BoxFnOnce::from(callback)));
    }

    /// Add a callback that will be called when the execution starts.
    pub fn on_execution_start<F>(&mut self, execution: &ExecutionUuid, callback: F)
    where
        F: (FnOnce(WorkerUuid) -> ()) + 'static,
    {
        self.execution_callback(execution)
            .on_start
            .push(BoxFnOnce::from(callback));
    }

    /// Add a callback that will be called when the execution ends.
    pub fn on_execution_done<F>(&mut self, execution: &ExecutionUuid, callback: F)
    where
        F: (FnOnce(ExecutionResult) -> ()) + 'static,
    {
        self.execution_callback(execution)
            .on_done
            .push(BoxFnOnce::from(callback));
    }

    /// Add a callback that will be called when the execution is skipped.
    pub fn on_execution_skip<F>(&mut self, execution: &ExecutionUuid, callback: F)
    where
        F: (FnOnce() -> ()) + 'static,
    {
        self.execution_callback(execution)
            .on_skip
            .push(BoxFnOnce::from(callback));
    }

    /// Makes sure that a callback item exists for that file and returns a &mut to it.
    fn file_callback<F: Into<FileUuid>>(&mut self, file: F) -> &mut FileCallbacks {
        self.file_callbacks.entry(file.into()).or_default()
    }

    /// Makes sure that a callback item exists for that execution and returns a &mut to it.
    fn execution_callback(&mut self, execution: &ExecutionUuid) -> &mut ExecutionCallbacks {
        self.execution_callbacks.entry(*execution).or_default()
    }
}