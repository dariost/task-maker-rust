use crate::score_types::*;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

/// The score type `min`, for each subtask takes the value of worst testcase
/// score.
#[derive(Debug)]
pub struct ScoreTypeMin<
    SubtaskId: Eq + PartialOrd + Hash + Copy + Debug,
    TestcaseId: Eq + PartialOrd + Hash + Copy + Debug,
> {
    /// The basic data managed by ScoreTypeBase.
    base: ScoreTypeBase<SubtaskId, TestcaseId>,
}

impl<SubtaskId, TestcaseId> ScoreTypeMin<SubtaskId, TestcaseId>
where
    SubtaskId: Eq + PartialOrd + Hash + Copy + Debug,
    TestcaseId: Eq + PartialOrd + Hash + Copy + Debug,
{
    /// Make a new ScoreTypeMin.
    pub fn new(
        subtasks: HashMap<SubtaskId, &SubtaskInfo>,
        testcases: HashMap<SubtaskId, HashMap<TestcaseId, &TestcaseInfo<SubtaskId, TestcaseId>>>,
    ) -> ScoreTypeMin<SubtaskId, TestcaseId> {
        ScoreTypeMin {
            base: ScoreTypeBase::new(subtasks, testcases),
        }
    }
}

impl<SubtaskId, TestcaseId> ScoreType<SubtaskId, TestcaseId> for ScoreTypeMin<SubtaskId, TestcaseId>
where
    SubtaskId: Eq + PartialOrd + Hash + Copy + Debug + 'static,
    TestcaseId: Eq + PartialOrd + Hash + Copy + Debug + 'static,
{
    fn testcase_score(&mut self, subtask: SubtaskId, testcase: TestcaseId, score: f64) -> bool {
        self.base.testcase_score(subtask, testcase, score);
        let mut score: f64 = 1.0;
        for testcase in self.base.testcase_scores.get(&subtask).unwrap().values() {
            // there is a non-ready testcase of this subtask
            if testcase.is_none() {
                return false;
            }
            score = score.min(testcase.unwrap());
        }
        // all the testcases are ready, update the subtask score
        score *= self.base.max_subtask_scores.get(&subtask).unwrap();
        *self.base.subtask_scores.get_mut(&subtask).unwrap() = Some(score);

        let mut score: f64 = 0.0;
        for subtask in self.base.subtask_scores.values() {
            // there is a non-ready subtask
            if subtask.is_none() {
                return true; // the subtask has been updated tho
            }
            score += subtask.unwrap();
        }
        // all the subtasks are ready, update the task
        self.base.task_score = Some(score);
        true
    }

    fn get_subtask_score(&mut self, subtask: SubtaskId) -> Option<f64> {
        self.base.get_subtask_score(subtask)
    }

    fn get_task_score(&mut self) -> Option<f64> {
        self.base.get_task_score()
    }

    fn clone(&self) -> Box<dyn ScoreType<SubtaskId, TestcaseId>> {
        Box::new(ScoreTypeMin {
            base: self.base.clone(),
        })
    }
}