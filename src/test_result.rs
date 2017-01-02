pub enum TestResult {
    OK,
    Skipped,
    Failed,
}

impl TestResult {
    pub fn from_status(status: i32) -> TestResult {
        match status {
            0 => TestResult::OK,
            89 => TestResult::Skipped,
            _ => TestResult::Failed,
        }
    }

    // Return a combined return status. If either side skipped, then
    // we mark it skipped. Otherwise we return OK only if both sides
    // reported OK.
    pub fn merge(a: TestResult, b: TestResult) -> TestResult {
        let res = (a, b);
        match res {
            (TestResult::Skipped, _) => TestResult::Skipped,
            (_, TestResult::Skipped) => TestResult::Skipped,
            (TestResult::Failed, _) => TestResult::Failed,
            (_, TestResult::Failed) => TestResult::Failed,
            (TestResult::OK, TestResult::OK) => TestResult::OK,
        }
    }
}
