use crate::ioi::*;
use crate::ui::{UIExecutionStatus, UIMessage};
use std::collections::HashMap;
use std::path::PathBuf;
use task_maker_dag::*;

/// The status of the compilation of a file.
#[derive(Debug)]
pub enum CompilationStatus {
    /// The compilation is known but it has not started yet.
    Pending,
    /// The compilation is running on a worker.
    Running,
    /// The compilation has completed.
    Done {
        /// The result of the compilation.
        result: ExecutionResult,
    },
    /// The compilation has failed.
    Failed {
        /// The result of the compilation.
        result: ExecutionResult,
    },
    /// The compilation has been skipped.
    Skipped,
}

/// Status of the generation of a testcase input and output.
#[derive(Debug)]
pub enum TestcaseGenerationStatus {
    /// The generation has not started yet.
    Pending,
    /// The input file is generating.
    Generating,
    /// The input file has been generated.
    Generated,
    /// The input file is being validated.
    Validating,
    /// The input file has been validated.
    Validated,
    /// The output file is generating.
    Solving,
    /// The output file has been generated.
    Solved,
    /// The generation of the testcase has failed.
    Failed,
    /// The generation has been skipped.
    Skipped,
}

/// Status of the evaluation of a solution on a testcase.
#[derive(Debug)]
pub enum TestcaseEvaluationStatus {
    /// The solution has not started yet.
    Pending,
    /// The solution is running.
    Solving,
    /// The solution exited successfully, waiting for the checker.
    Solved,
    /// Checker is running.
    Checking,
    /// The solution scored 100% of the testcase.
    Accepted(String),
    /// The output is wrong.
    WrongAnswer(String),
    /// The solution is partially correct.
    Partial(String),
    /// The solution timed out.
    TimeLimitExceeded,
    /// The solution exceeded the wall time limit.
    WallTimeLimitExceeded,
    /// The solution exceeded the memory limit.
    MemoryLimitExceeded,
    /// The solution crashed.
    RuntimeError,
    /// Something went wrong.
    Failed,
    /// The evaluation has been skipped.
    Skipped,
}

/// State of the generation of a testcases.
#[derive(Debug)]
pub struct TestcaseGenerationState {
    /// Status of the generation.
    pub status: TestcaseGenerationStatus,
    /// Result of the generation.
    pub generation: Option<ExecutionResult>,
    /// Result of the validation.
    pub validation: Option<ExecutionResult>,
    /// Result of the solution.
    pub solution: Option<ExecutionResult>,
}

/// State of the generation of a subtask.
#[derive(Debug)]
pub struct SubtaskGenerationState {
    /// State of the testcases of this subtask.
    pub testcases: HashMap<TestcaseId, TestcaseGenerationState>,
}

/// State of the evaluation of a testcase.
#[derive(Debug)]
pub struct SolutionTestcaseEvaluationState {
    /// The score on that testcase
    pub score: Option<f64>,
    /// The status of the execution.
    pub status: TestcaseEvaluationStatus,
    /// The result of the solution.
    pub result: Option<ExecutionResult>,
    /// The result of the checker.
    pub checker: Option<ExecutionResult>,
}

/// State of the evaluation of a subtask.
#[derive(Debug)]
pub struct SolutionSubtaskEvaluationState {
    /// Score of the subtask.
    pub score: Option<f64>,
    /// The state of the evaluation of the testcases.
    pub testcases: HashMap<TestcaseId, SolutionTestcaseEvaluationState>,
}

/// State of the evaluation of a solution.
#[derive(Debug)]
pub struct SolutionEvaluationState {
    /// Score of the solution.
    pub score: Option<f64>,
    /// The state of the evaluation of the subtasks.
    pub subtasks: HashMap<SubtaskId, SolutionSubtaskEvaluationState>,
}

impl SolutionEvaluationState {
    /// Make a new, empty, `SolutionEvaluationState`.
    pub fn new(task: &Task) -> SolutionEvaluationState {
        SolutionEvaluationState {
            score: None,
            subtasks: task
                .subtasks
                .values()
                .map(|subtask| {
                    (
                        subtask.id,
                        SolutionSubtaskEvaluationState {
                            score: None,
                            testcases: subtask
                                .testcases
                                .values()
                                .map(|testcase| {
                                    (
                                        testcase.id,
                                        SolutionTestcaseEvaluationState {
                                            score: None,
                                            status: TestcaseEvaluationStatus::Pending,
                                            result: None,
                                            checker: None,
                                        },
                                    )
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
        }
    }
}

/// The state of a IOI task, all the information for the UI are stored here.
#[derive(Debug)]
pub struct UIState {
    /// The task.
    pub task: Task,
    /// The maximum score of this task.
    pub max_score: f64,
    /// The status of the compilations.
    pub compilations: HashMap<PathBuf, CompilationStatus>,
    /// The state of the generation of the testcases.
    pub generations: HashMap<SubtaskId, SubtaskGenerationState>,
    /// The status of the evaluations of the solutions.
    pub evaluations: HashMap<PathBuf, SolutionEvaluationState>,
}

impl TestcaseEvaluationStatus {
    /// Whether the testcase evaluation has completed, either successfully or not.
    pub fn has_completed(&self) -> bool {
        match self {
            TestcaseEvaluationStatus::Pending
            | TestcaseEvaluationStatus::Solving
            | TestcaseEvaluationStatus::Solved
            | TestcaseEvaluationStatus::Checking => false,
            _ => true,
        }
    }

    /// Whether the testcase evaluation has completed successfully.
    pub fn is_success(&self) -> bool {
        match self {
            TestcaseEvaluationStatus::Accepted(_) => true,
            _ => false,
        }
    }

    /// Whether the testcase evaluation has completed with a partial score.
    pub fn is_partial(&self) -> bool {
        match self {
            TestcaseEvaluationStatus::Partial(_) => true,
            _ => false,
        }
    }

    pub fn message(&self) -> String {
        use TestcaseEvaluationStatus::*;
        return match self {
            Pending => "Not done".into(),
            Solving => "Solution running".into(),
            Solved => "Solution completed".into(),
            Checking => "Checker running".into(),
            Accepted(s) => {
                if s.is_empty() {
                    "Output is correct".into()
                } else {
                    s.clone()
                }
            }
            WrongAnswer(s) => {
                if s.is_empty() {
                    "Output is not correct".into()
                } else {
                    s.clone()
                }
            }
            Partial(s) => {
                if s.is_empty() {
                    "Partially correct".into()
                } else {
                    s.clone()
                }
            }
            TimeLimitExceeded => "Time limit exceeded".into(),
            WallTimeLimitExceeded => "Execution took too long".into(),
            MemoryLimitExceeded => "Memory limit exceeded".into(),
            RuntimeError => "Runtime error".into(),
            Failed => "Execution failed".into(),
            Skipped => "Execution skipped".into(),
        };
    }
}

impl UIState {
    /// Make a new `UIState`.
    pub fn new(task: &Task) -> UIState {
        let generations = task
            .subtasks
            .iter()
            .map(|(st_num, subtask)| {
                (
                    *st_num,
                    SubtaskGenerationState {
                        testcases: subtask
                            .testcases
                            .iter()
                            .map(|(tc_num, _)| {
                                (
                                    *tc_num,
                                    TestcaseGenerationState {
                                        status: TestcaseGenerationStatus::Pending,
                                        generation: None,
                                        validation: None,
                                        solution: None,
                                    },
                                )
                            })
                            .collect(),
                    },
                )
            })
            .collect();
        UIState {
            max_score: task.subtasks.values().map(|s| s.max_score).sum(),
            task: task.clone(),
            compilations: HashMap::new(),
            generations,
            evaluations: HashMap::new(),
        }
    }

    /// Apply a `UIMessage` to this state.
    pub fn apply(&mut self, message: UIMessage) {
        match message {
            UIMessage::Compilation { file, status } => {
                let comp = self
                    .compilations
                    .entry(file.clone())
                    .or_insert(CompilationStatus::Pending);
                match status {
                    UIExecutionStatus::Pending => *comp = CompilationStatus::Pending,
                    UIExecutionStatus::Started { .. } => *comp = CompilationStatus::Running,
                    UIExecutionStatus::Done { result } => {
                        if let ExecutionStatus::Success = result.status {
                            *comp = CompilationStatus::Done { result };
                        } else {
                            *comp = CompilationStatus::Failed { result };
                        }
                    }
                    UIExecutionStatus::Skipped => *comp = CompilationStatus::Skipped,
                }
            }
            UIMessage::IOIGeneration {
                subtask,
                testcase,
                status,
            } => {
                let gen = self
                    .generations
                    .get_mut(&subtask)
                    .unwrap()
                    .testcases
                    .get_mut(&testcase)
                    .unwrap();
                match status {
                    UIExecutionStatus::Pending => gen.status = TestcaseGenerationStatus::Pending,
                    UIExecutionStatus::Started { .. } => {
                        gen.status = TestcaseGenerationStatus::Generating
                    }
                    UIExecutionStatus::Done { result } => {
                        if let ExecutionStatus::Success = result.status {
                            gen.status = TestcaseGenerationStatus::Generated;
                        } else {
                            gen.status = TestcaseGenerationStatus::Failed;
                        }
                        gen.generation = Some(result);
                    }
                    UIExecutionStatus::Skipped => gen.status = TestcaseGenerationStatus::Skipped,
                }
            }
            UIMessage::IOIValidation {
                subtask,
                testcase,
                status,
            } => {
                let gen = self
                    .generations
                    .get_mut(&subtask)
                    .unwrap()
                    .testcases
                    .get_mut(&testcase)
                    .unwrap();
                match status {
                    UIExecutionStatus::Pending => gen.status = TestcaseGenerationStatus::Pending,
                    UIExecutionStatus::Started { .. } => {
                        gen.status = TestcaseGenerationStatus::Validating
                    }
                    UIExecutionStatus::Done { result } => {
                        if let ExecutionStatus::Success = result.status {
                            gen.status = TestcaseGenerationStatus::Validated;
                        } else {
                            gen.status = TestcaseGenerationStatus::Failed;
                        }
                        gen.validation = Some(result);
                    }
                    UIExecutionStatus::Skipped => {
                        if let TestcaseGenerationStatus::Failed = gen.status {
                        } else {
                            gen.status = TestcaseGenerationStatus::Skipped;
                        }
                    }
                }
            }
            UIMessage::IOISolution {
                subtask,
                testcase,
                status,
            } => {
                let gen = self
                    .generations
                    .get_mut(&subtask)
                    .unwrap()
                    .testcases
                    .get_mut(&testcase)
                    .unwrap();
                match status {
                    UIExecutionStatus::Pending => gen.status = TestcaseGenerationStatus::Pending,
                    UIExecutionStatus::Started { .. } => {
                        gen.status = TestcaseGenerationStatus::Solving
                    }
                    UIExecutionStatus::Done { result } => {
                        if let ExecutionStatus::Success = result.status {
                            gen.status = TestcaseGenerationStatus::Solved;
                        } else {
                            gen.status = TestcaseGenerationStatus::Failed;
                        }
                        gen.solution = Some(result);
                    }
                    UIExecutionStatus::Skipped => {
                        if let TestcaseGenerationStatus::Failed = gen.status {
                        } else {
                            gen.status = TestcaseGenerationStatus::Skipped;
                        }
                    }
                }
            }
            UIMessage::IOIEvaluation {
                subtask,
                testcase,
                solution,
                status,
            } => {
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert(SolutionEvaluationState::new(&self.task));
                let subtask = eval.subtasks.get_mut(&subtask).expect("Missing subtask");
                let mut testcase = subtask
                    .testcases
                    .get_mut(&testcase)
                    .expect("Missing testcase");
                match status {
                    UIExecutionStatus::Pending => {}
                    UIExecutionStatus::Started { .. } => {
                        testcase.status = TestcaseEvaluationStatus::Solving
                    }
                    UIExecutionStatus::Done { result } => {
                        match result.status {
                            ExecutionStatus::Success => {
                                testcase.status = TestcaseEvaluationStatus::Solved
                            }
                            ExecutionStatus::ReturnCode(_) => {
                                testcase.status = TestcaseEvaluationStatus::RuntimeError
                            }
                            ExecutionStatus::Signal(_, _) => {
                                testcase.status = TestcaseEvaluationStatus::RuntimeError
                            }
                            ExecutionStatus::TimeLimitExceeded => {
                                testcase.status = TestcaseEvaluationStatus::TimeLimitExceeded
                            }
                            ExecutionStatus::SysTimeLimitExceeded => {
                                testcase.status = TestcaseEvaluationStatus::TimeLimitExceeded
                            }
                            ExecutionStatus::WallTimeLimitExceeded => {
                                testcase.status = TestcaseEvaluationStatus::WallTimeLimitExceeded
                            }
                            ExecutionStatus::MemoryLimitExceeded => {
                                testcase.status = TestcaseEvaluationStatus::MemoryLimitExceeded
                            }
                            ExecutionStatus::InternalError(_) => {
                                testcase.status = TestcaseEvaluationStatus::Failed
                            }
                        }
                        testcase.result = Some(result);
                    }
                    UIExecutionStatus::Skipped => {
                        testcase.status = TestcaseEvaluationStatus::Skipped
                    }
                }
            }
            UIMessage::IOIChecker {
                subtask,
                testcase,
                solution,
                status,
            } => {
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert(SolutionEvaluationState::new(&self.task));
                let subtask = eval.subtasks.get_mut(&subtask).expect("Missing subtask");
                let mut testcase = subtask
                    .testcases
                    .get_mut(&testcase)
                    .expect("Missing testcase");
                match status {
                    UIExecutionStatus::Started { .. } => {
                        testcase.status = TestcaseEvaluationStatus::Checking;
                    }
                    UIExecutionStatus::Done { result } => {
                        testcase.checker = Some(result);
                    }
                    _ => {}
                }
            }
            UIMessage::IOITestcaseScore {
                subtask,
                testcase,
                solution,
                score,
                message,
            } => {
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert(SolutionEvaluationState::new(&self.task));
                let subtask = eval.subtasks.get_mut(&subtask).expect("Missing subtask");
                let mut testcase = subtask
                    .testcases
                    .get_mut(&testcase)
                    .expect("Missing testcase");
                testcase.score = Some(score);
                if !testcase.status.has_completed() {
                    if score == 0.0 {
                        testcase.status = TestcaseEvaluationStatus::WrongAnswer(message);
                    } else if (score - 1.0).abs() < 0.001 {
                        testcase.status = TestcaseEvaluationStatus::Accepted(message);
                    } else {
                        testcase.status = TestcaseEvaluationStatus::Partial(message);
                    }
                }
            }
            UIMessage::IOISubtaskScore {
                subtask,
                solution,
                score,
            } => {
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert(SolutionEvaluationState::new(&self.task));
                let mut subtask = eval.subtasks.get_mut(&subtask).expect("Missing subtask");
                subtask.score = Some(score);
            }
            UIMessage::IOITaskScore { solution, score } => {
                let eval = self
                    .evaluations
                    .entry(solution)
                    .or_insert(SolutionEvaluationState::new(&self.task));
                eval.score = Some(score);
            }
            _ => {}
        }
    }
}