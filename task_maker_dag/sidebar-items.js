initSidebarItems({"enum":[["CacheMode","The setting of the cache level."],["ExecutionCommand","Command of an `Execution` to execute."],["ExecutionStatus","Status of a completed `Execution`."],["ProvidedFile","A wrapper around a `File` provided by the client, this means that the client knows the `FileStoreKey` and the path to that file if it's local, or it's content if it's generated."]],"struct":[["Execution","An `Execution` is a process that will be executed by a worker inside a sandbox. The sandbox will limit the execution of the process (e.g. killing it after a time limit occurs, or preventing it from reading/writing files)."],["ExecutionCallbacks","The callbacks to be called when an event of an execution occurs."],["ExecutionDAG","A computation DAG, this is not serializable because it contains the callbacks of the client."],["ExecutionDAGConfig","Configuration setting of an `ExecutionDAG`, some of the values set here will be inherited in the configuration of the executions added."],["ExecutionDAGData","Serializable part of the execution DAG: everything except the callbacks (which are not serializable)."],["ExecutionInput","An input file of an `Execution`, can be marked as executable if it has to be run inside the sandbox."],["ExecutionLimits","Limits on an `Execution`. On some worker platforms some of the fields may not be supported or may be less accurate."],["ExecutionResourcesUsage","Resources used during the execution, note that on some platform these values may not be accurate."],["ExecutionResult","The result of an `Execution`."],["ExecutionTag","A tag on an `Execution`. Can be used to classify the executions into groups and refer to them, for example for splitting the cache scopes."],["File","An handle to a file in the evaluation, this only tracks dependencies between executions."],["FileCallbacks","The callbacks that will trigger when the file is ready."],["WriteToCallback","Where to write the file to with some other information."]],"type":[["ExecutionUuid","The identifier of an execution, it's globally unique and it identifies an execution only during a single evaluation."],["FileUuid","The identifier of a file, it's globally unique and it identifies a file only during a single evaluation."],["GetContentCallback","Type of the callback called when a file is returned to the client."],["OnDoneCallback","Type of the callback called when an `Execution` ends."],["OnSkipCallback","Type of the callback called when an `Execution` is skipped."],["OnStartCallback","Type of the callback called when an `Execution` starts."],["Priority","Type of the priority value of an `Execution`."],["WorkerUuid","The identifier of a worker, it's globally unique and identifies the worker during a single connection. It is used to associate the jobs to the workers which runs the executions. The underlying executor may provide more information about a worker using this id."]]});