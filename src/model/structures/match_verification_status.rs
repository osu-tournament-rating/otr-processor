use serde_repr::{Deserialize_repr, Serialize_repr};
use std::convert::TryFrom;

/// Source: https://github.com/osu-tournament-rating/otr-api/blob/master/API/Enums/VerificationEnums.cs#L18
#[derive(Deserialize_repr, Serialize_repr, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum MatchVerificationStatus {
    #[default]
    Verified = 0,
    PendingVerification = 1,
    PreVerified = 2,
    Rejected = 3,
    Unknown = 4,
    Failure = 5
}

impl TryFrom<i32> for MatchVerificationStatus {
    type Error = ();

    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(MatchVerificationStatus::Verified),
            1 => Ok(MatchVerificationStatus::PendingVerification),
            2 => Ok(MatchVerificationStatus::PreVerified),
            3 => Ok(MatchVerificationStatus::Rejected),
            4 => Ok(MatchVerificationStatus::Unknown),
            5 => Ok(MatchVerificationStatus::Failure),
            _ => Err(())
        }
    }
}
