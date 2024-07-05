use serde_repr::{Deserialize_repr, Serialize_repr};
use std::convert::TryFrom;
use strum_macros::EnumIter;

#[derive(Deserialize_repr, Serialize_repr, Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
#[repr(u8)]
pub enum Ruleset {
    Osu = 0,
    Taiko = 1,
    Catch = 2,
    Mania4k = 3,
    Mania7k = 4
}

impl TryFrom<i32> for Ruleset {
    type Error = ();

    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Ruleset::Osu),
            1 => Ok(Ruleset::Taiko),
            2 => Ok(Ruleset::Catch),
            3 => Ok(Ruleset::Mania4k),
            4 => Ok(Ruleset::Mania7k),
            _ => Err(())
        }
    }
}
