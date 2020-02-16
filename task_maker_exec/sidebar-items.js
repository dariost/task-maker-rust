initSidebarItems({"enum":[["ChannelMessage","Message type that can be send in a channel."],["ChannelReceiver","The channel part that receives data."],["ChannelSender","The channel part that sends data."],["RawSandboxResult","Response of the internal implementation of the sandbox."]],"fn":[["connect_channel","Connect to a remote channel."],["eval_dag_locally","Evaluate a DAG locally spawning a new `LocalExecutor` with the specified number of workers."],["new_local_channel","Make a new pair of `ChannelSender` / `ChannelReceiver`"]],"mod":[["executors","The supported executors."],["proto","The protocol related structs and enums."]],"struct":[["ChannelServer","Listener for connections on some TCP socket."],["ErrorSandboxRunner","A fake sandbox that don't actually spawn anything and always return an error."],["ExecutorClient","This is a client of the `Executor`, the client is who sends a DAG for an evaluation, provides some files and receives the callbacks from the server. When the server notifies a callback function is called by the client."],["ExecutorStatus","The current status of the `Executor`, this is sent to the user when the server status is asked."],["ExecutorWorkerStatus","Status of a worker of an `Executor`."],["FakeSandboxRunner","A fake sandbox that don't actually spawn anything and return with success, if the command was `true` the exit code is zero, otherwise it's 1."],["SuccessSandboxRunner","A fake sandbox that don't actually spawn anything and always return successfully with exit code 0."],["Worker","The worker is the component that receives the work from the server and sends the results back. It computes the results by executing a process inside a sandbox, limiting the available resources and measuring the used ones."],["WorkerConn","An handle of the connection to the worker."]],"trait":[["SandboxRunner","Something able to spawn a sandbox, wait for it to exit and return the results."]]});