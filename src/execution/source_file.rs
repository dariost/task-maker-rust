use crate::evaluation::*;
use crate::execution::*;
use crate::languages::*;
use crate::ui::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A source file that will be able to be execute (with an optional
/// compilation step).
#[derive(Debug)]
pub struct SourceFile {
    /// Path to the source file.
    pub path: PathBuf,
    /// Language of the source file.
    pub language: Arc<Language>,
    /// Handle to the executable after the compilation/provided file.
    pub executable: Arc<Mutex<Option<File>>>,
}

impl SourceFile {
    /// Make a new SourceFile from the provided file. Will return None if the
    /// language is unknown.
    pub fn new(path: &Path) -> Option<SourceFile> {
        let lang = LanguageManager::detect_language(path);
        lang.as_ref()?;
        Some(SourceFile {
            path: path.to_owned(),
            language: lang.unwrap(),
            executable: Arc::new(Mutex::new(None)),
        })
    }

    /// Execute the program relative to this source file with the specified
    /// args. If the file has not been compiled yet this may add the
    /// compilation to the dag.
    ///
    /// The returned execution has all the dependencies already set, but it has
    /// not been added to the DAG yet.
    pub fn execute(
        &self,
        eval: &mut EvaluationData,
        description: &str,
        args: Vec<String>,
    ) -> Execution {
        self.prepare(eval);
        let mut exec = Execution::new(description, self.language.runtime_command(&self.path));
        exec.args = self.language.runtime_args(&self.path, args);
        exec.input(
            self.executable.lock().unwrap().as_ref().unwrap(),
            &self.language.executable_name(&self.path),
            true,
        );
        for dep in self.language.runtime_dependencies(&self.path) {
            exec.input(&dep.file, &dep.sandbox_path, dep.executable);
            eval.dag.provide_file(dep.file, &dep.local_path);
        }
        exec
    }

    /// The name of the source file, it's based on the name of the file.
    pub fn name(&self) -> String {
        String::from(self.path.file_name().unwrap().to_str().unwrap())
    }

    /// Prepare the source file setting the `executable` and eventually
    /// compiling the source file.
    fn prepare(&self, eval: &mut EvaluationData) {
        if self.executable.lock().unwrap().is_some() {
            return;
        }
        if self.language.need_compilation() {
            let mut comp = Execution::new(
                &format!("Compilation of {:?}", self.name()),
                self.language.compilation_command(&self.path),
            );
            comp.args = self.language.compilation_args(&self.path);
            let source = File::new(&format!("Source file of {:?}", self.path));
            comp.input(&source, Path::new(self.path.file_name().unwrap()), false);
            comp.limits.nproc = None;
            for dep in self.language.compilation_dependencies(&self.path) {
                comp.input(&dep.file, &dep.sandbox_path, dep.executable);
                eval.dag.provide_file(dep.file, &dep.local_path);
            }
            let exec = comp.output(&self.language.executable_name(&self.path));
            eval.dag.provide_file(source, &self.path);
            let (sender1, path1) = (eval.sender.clone(), self.name());
            let (sender2, path2) = (eval.sender.clone(), self.name());
            let (sender3, path3) = (eval.sender.clone(), self.name());
            eval.dag.on_execution_start(&comp.uuid, move |worker| {
                sender1
                    .send(UIMessage::Compilation {
                        file: PathBuf::from(path1),
                        status: UIExecutionStatus::Started {
                            worker: worker.to_string(),
                        },
                    })
                    .unwrap();
            });
            eval.dag.on_execution_done(&comp.uuid, move |result| {
                sender2
                    .send(UIMessage::Compilation {
                        file: PathBuf::from(path2),
                        status: UIExecutionStatus::Done { result },
                    })
                    .unwrap();
            });
            eval.dag.on_execution_skip(&comp.uuid, move || {
                sender3
                    .send(UIMessage::Compilation {
                        file: PathBuf::from(path3),
                        status: UIExecutionStatus::Skipped,
                    })
                    .unwrap();
            });
            eval.dag.add_execution(comp);
            eval.sender
                .send(UIMessage::Compilation {
                    file: PathBuf::from(self.name()),
                    status: UIExecutionStatus::Pending,
                })
                .unwrap();
            // TODO bind the compilation events
            *self.executable.lock().unwrap() = Some(exec);
        } else {
            let executable = File::new(&format!("Source file of {:?}", self.path));
            *self.executable.lock().unwrap() = Some(executable.clone());
            eval.dag.provide_file(executable, &self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_source_file_cpp() {
        let cwd = setup_test();

        let (mut eval, _receiver) = EvaluationData::new();
        let source = "int main() {return 0;}";
        let source_path = cwd.path().join("source.cpp");
        std::fs::File::create(&source_path)
            .unwrap()
            .write_all(source.as_bytes())
            .unwrap();
        let source = SourceFile::new(&source_path).unwrap();
        let exec = source.execute(&mut eval, "Testing exec", vec![]);

        let exec_start = Arc::new(AtomicBool::new(false));
        let exec_start2 = exec_start.clone();
        let exec_done = Arc::new(AtomicBool::new(false));
        let exec_done2 = exec_done.clone();
        let exec_skipped = Arc::new(AtomicBool::new(false));
        let exec_skipped2 = exec_skipped.clone();
        eval.dag.on_execution_start(&exec.uuid, move |_w| {
            exec_start.store(true, Ordering::Relaxed)
        });
        eval.dag.on_execution_done(&exec.uuid, move |_res| {
            exec_done.store(true, Ordering::Relaxed)
        });
        eval.dag.on_execution_skip(&exec.uuid, move || {
            exec_skipped.store(true, Ordering::Relaxed)
        });
        eval.dag.add_execution(exec);

        eval_dag_locally(eval, cwd.path());

        assert!(exec_start2.load(Ordering::Relaxed));
        assert!(exec_done2.load(Ordering::Relaxed));
        assert!(!exec_skipped2.load(Ordering::Relaxed));
    }
}
