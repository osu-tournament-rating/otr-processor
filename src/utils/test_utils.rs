pub struct TestConstants {
    pub open_skill_leniency: f64
}

impl TestConstants {
    pub fn new() -> TestConstants {
        TestConstants {
            open_skill_leniency: 0.000000001
        }
    }
}
