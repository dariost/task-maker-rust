use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use failure::{bail, format_err, Error};

use task_maker_cache::Cache;
use task_maker_dag::CacheMode;
use task_maker_exec::executors::{LocalExecutor, RemoteEntityMessage};
use task_maker_exec::{connect_channel, new_local_channel, ExecutorClient};
use task_maker_format::ui::{UIMessage, UIType, UI};
use task_maker_format::{EvaluationData, TaskFormat, UISender, VALID_TAGS};
use task_maker_store::FileStore;

use crate::detect_format::find_task;
use crate::error::NiceError;
use crate::opt::Opt;
use crate::sandbox::SelfExecSandboxRunner;
use task_maker_exec::proto::ExecutorClientMessage;

/// The result of an evaluation.
pub enum Evaluation {
    /// The evaluation has completed.
    Done,
    /// The task directory has been cleaned.
    Clean,
}

/// Run the local evaluation of some actions (either building a task or cleaning its directory).
///
/// The instructions on what to do are expressed via the "command line" options passed as arguments.
/// This method will block until the execution ends, but it accepts a function as parameter used as
/// callback for when UI messages are produced.
///
/// The callback takes 2 parameters, a reference to the current UI and the message produced.
/// Typically the only thing that function should do is to send the message to the UI, this
/// behaviour can be changed.
///
/// ```no_run
/// # use structopt::StructOpt;
/// # use task_maker_rust::local::run_evaluation;
/// # let opt = task_maker_rust::opt::Opt::from_args();
/// run_evaluation(opt, move |ui, mex| ui.on_message(mex));
/// ```
pub fn run_evaluation<F>(opt: Opt, mut on_message: F) -> Result<Evaluation, Error>
where
    F: 'static + FnMut(&mut dyn UI, UIMessage) -> () + Send,
{
    if opt.exclusive {
        bail!("This option is not implemented yet");
    }

    // setup the task
    let eval_config = opt.to_config();
    let task: Box<dyn TaskFormat> = find_task(&opt.task_dir, opt.max_depth, &eval_config)
        .map_err(|e| format_err!("Invalid task directory: {}", e.to_string()))?;

    if opt.task_info {
        match opt.ui {
            UIType::Json => {
                println!("{}", serde_json::to_string(&task.task_info()?)?);
            }
            _ => {
                println!("{:#?} ", task.task_info()?);
            }
        }
        return Ok(Evaluation::Done);
    }

    // clean the task
    if opt.clean {
        task.clean()
            .map_err(|e| format_err!("Cannot clear the task directory: {}", e.to_string()))?;
        return Ok(Evaluation::Clean);
    }

    // setup the configuration and the evaluation metadata
    let (mut eval, ui_receiver) = EvaluationData::new(task.path());
    let config = eval.dag.config_mut();
    config
        .keep_sandboxes(opt.keep_sandboxes)
        .dry_run(opt.dry_run)
        .cache_mode(CacheMode::try_from(&opt.no_cache, &VALID_TAGS)?)
        .copy_exe(opt.copy_exe)
        .copy_logs(opt.copy_logs);
    if let Some(extra_time) = opt.extra_time {
        if extra_time < 0.0 {
            bail!("The extra time ({}) cannot be negative!", extra_time);
        }
        config.extra_time(extra_time);
    }

    // setup the file store
    let store_path = opt.store_dir();
    let file_store = Arc::new(
        FileStore::new(
            store_path.join("store"),
            opt.max_cache * 1024 * 1024,
            opt.min_cache * 1024 * 1024,
        )
        .map_err(|e| {
            format_err!(
                "Cannot create the file store: {}\nYou can try wiping it with --dont-panic",
                e.to_string()
            )
        })?,
    );

    // setup the ui thread
    let mut ui = task
        .ui(&opt.ui)
        .map_err(|_| format_err!("This UI is not supported on this task type."))?;
    let ui_thread = std::thread::Builder::new()
        .name("UI".to_owned())
        .spawn(move || {
            while let Ok(message) = ui_receiver.recv() {
                if let UIMessage::StopUI = message {
                    break;
                }
                on_message(ui.as_mut(), message);
            }
            ui.finish();
        })
        .map_err(|e| format_err!("Failed to spawn UI thread: {}", e.to_string()))?;

    // setup the executor
    let cache = Cache::new(store_path.join("cache"))
        .map_err(|e| format_err!("Cannot create the cache: {}", e.to_string()))?;
    let num_cores = opt.num_cores.unwrap_or_else(num_cpus::get);
    let sandbox_path = store_path.join("sandboxes");
    let executor = LocalExecutor::new(file_store.clone(), num_cores, sandbox_path);

    // build the DAG for the task
    if let Err(e) = task.build_dag(&mut eval, &eval_config) {
        eval.sender.send(UIMessage::StopUI)?;
        ui_thread
            .join()
            .map_err(|e| format_err!("UI panicked: {:?}", e))?;
        bail!("Cannot build task DAG! {:?}", e);
    }

    trace!("The DAG is: {:#?}", eval.dag);

    let (tx, rx, server) = if let Some(evaluate_on) = opt.evaluate_on {
        let server_addr = SocketAddr::from_str(&evaluate_on)
            .map_err(|_| format_err!("Invalid server address provided"))?;
        let (tx, rx) = connect_channel(server_addr)
            .map_err(|e| format_err!("Failed to connect to the server: {}", e.to_string()))?;
        let name = opt
            .name
            .unwrap_or_else(|| format!("{}@{}", whoami::username(), whoami::hostname()));
        tx.send(RemoteEntityMessage::Welcome { name })
            .map_err(|e| format_err!("Cannot send welcome to the server: {}", e.to_string()))?;
        (tx.change_type(), rx.change_type(), None)
    } else {
        // start the server and the client
        let (tx, rx_remote) = new_local_channel();
        let (tx_remote, rx) = new_local_channel();
        let server = std::thread::Builder::new()
            .name("Executor thread".into())
            .spawn(move || {
                executor
                    .evaluate(
                        tx_remote,
                        rx_remote,
                        cache,
                        SelfExecSandboxRunner::default(),
                    )
                    .unwrap();
            })
            .map_err(|e| format_err!("Failed to spawn the executor thread: {}", e.to_string()))?;
        (tx, rx, Some(server))
    };

    let ui_sender = eval.sender.clone();
    // a shared sender for the ctrl-c handler, it has to be wrapped in Arc-Mutex-Option to be freed
    // at the end of the computation to allow the client to exit.
    let client_sender = Arc::new(Mutex::new(Some(tx.clone())));
    // `ctrlc` crate doesn't allow multiple calls of set_handler, and the tests may call this
    // function multiple times, so in the tests ^C handler is disabled.
    #[cfg(not(test))]
    {
        let client_sender_ctrlc = client_sender.clone();
        if let Err(e) = ctrlc::set_handler(move || {
            let sender = client_sender_ctrlc.lock().unwrap();
            if let Some(sender) = sender.as_ref() {
                if sender.send(ExecutorClientMessage::Stop).is_err() {
                    error!("Cannot tell the server to stop");
                }
            }
        }) {
            warn!("Cannot bind control-C handler: {:?}", e);
        }
    }
    // run the actual computation and block until it ends
    ExecutorClient::evaluate(eval.dag, tx, &rx, file_store, move |status| {
        ui_sender.send(UIMessage::ServerStatus { status })
    })
    .map_err(|e| {
        client_sender.lock().unwrap().as_ref().map(|tx| {
            let _ = tx.send(ExecutorClientMessage::Stop);
        });
        format_err!("Client failed: {}", e.to_string())
    })?;
    // disable the ctrl-c handler dropping the owned clone of the sender, letting the client exit
    client_sender.lock().unwrap().take();

    task.sanity_check_post_hook(&mut eval.sender.lock().unwrap())
        .map_err(|e| format_err!("Sanity checks failed: {}", e.to_string()))?;

    // wait for the server and the ui to exit
    if let Some(server) = server {
        server
            .join()
            .map_err(|e| format_err!("Executor panicked: {:?}", e))?;
    }
    drop(eval.sender); // make the UI exit
    ui_thread
        .join()
        .map_err(|e| format_err!("UI panicked: {:?}", e))?;
    Ok(Evaluation::Done)
}

/// Entry point of the local execution.
pub fn main_local(opt: Opt) {
    run_evaluation(opt, |ui, mex| ui.on_message(mex))
        .nice_expect_with(|e| format!("Error: {}", e.to_string()));
}
