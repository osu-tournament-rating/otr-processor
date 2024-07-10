use serde_repr::{Deserialize_repr, Serialize_repr};
use std::convert::TryFrom;

#[derive(Deserialize_repr, Serialize_repr, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RatingAdjustmentType {
    Initial = 0,
    Match = 1,
    Decay = 2
}

impl TryFrom<i32> for RatingAdjustmentType {
    type Error = ();
    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(RatingAdjustmentType::Initial),
            1 => Ok(RatingAdjustmentType::Match),
            2 => Ok(RatingAdjustmentType::Decay),
            _ => Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::structures::rating_adjustment_type;
    use rating_adjustment_type::RatingAdjustmentType;

    #[test]
    fn test_convert_initial() {
        assert_eq!(RatingAdjustmentType::try_from(0), Ok(RatingAdjustmentType::Initial));
    }

    #[test]
    fn test_convert_match() {
        assert_eq!(RatingAdjustmentType::try_from(1), Ok(RatingAdjustmentType::Match));
    }

    #[test]
    fn test_convert_decay() {
        assert_eq!(RatingAdjustmentType::try_from(2), Ok(RatingAdjustmentType::Decay));
    }

    #[test]
    fn test_convert_error() {
        assert_eq!(RatingAdjustmentType::try_from(3), Err(()));
    }
}
