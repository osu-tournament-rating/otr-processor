use serde_repr::{Deserialize_repr, Serialize_repr};

// score = 0, accuracy = 1, combo = 2, score v2 = 3
#[derive(Deserialize_repr, Serialize_repr, Debug)]
#[repr(u8)]
pub enum ScoringType {
    Score = 0,
    Accuracy = 1,
    Combo = 2,
    ScoreV2 = 3,
}
