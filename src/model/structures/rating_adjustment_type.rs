use serde_repr::{Deserialize_repr, Serialize_repr};
use std::convert::TryFrom;

#[derive(Deserialize_repr, Serialize_repr, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RatingSource { // AKA RatingAdjustmentType
    Decay = 0,
    Match = 1
}

impl TryFrom<i32> for RatingSource {
    type Error = ();
    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(RatingSource::Decay),
            1 => Ok(RatingSource::Match),
            _ => Err(())
        }
    }
}
