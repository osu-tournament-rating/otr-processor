use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Deserialize_repr, Serialize_repr, Debug, Eq, PartialEq, Copy, Clone, Hash)]
#[repr(u8)]
pub enum TeamType {
    HeadToHead = 0,
    TagCoop = 1,
    TeamVs = 2,
    TagTeamVs = 3
}

impl From<TeamType> for i32 {
    fn from(team_type: TeamType) -> Self {
        match team_type {
            TeamType::HeadToHead => 0,
            TeamType::TagCoop => 1,
            TeamType::TeamVs => 2,
            TeamType::TagTeamVs => 3
        }
    }
}
