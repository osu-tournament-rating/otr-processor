use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MatchType {
    Team = 1,
    HeadToHead = 2
}

impl TryFrom<i32> for MatchType {
    type Error = ();

    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            1 => Ok(MatchType::Team),
            2 => Ok(MatchType::HeadToHead),
            _ => Err(())
        }
    }
}
