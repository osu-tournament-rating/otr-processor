use serde_repr::{Deserialize_repr, Serialize_repr};
use std::convert::TryFrom;

#[derive(Deserialize_repr, Serialize_repr, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RatingAdjustmentType {
    Decay = 0,
    Match = 1,
    Initial = 2
}

impl TryFrom<i32> for RatingAdjustmentType {
    type Error = ();
    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(RatingAdjustmentType::Decay),
            1 => Ok(RatingAdjustmentType::Match),
            _ => Err(())
        }
    }
}
