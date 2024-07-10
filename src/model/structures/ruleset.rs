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

#[cfg(test)]
mod tests {
    use crate::model::structures::ruleset::Ruleset;
    use strum::IntoEnumIterator;

    #[test]
    fn test_convert_osu() {
        assert_eq!(Ruleset::try_from(0), Ok(Ruleset::Osu));
    }

    #[test]
    fn test_convert_taiko() {
        assert_eq!(Ruleset::try_from(1), Ok(Ruleset::Taiko));
    }

    #[test]
    fn test_convert_catch() {
        assert_eq!(Ruleset::try_from(2), Ok(Ruleset::Catch));
    }

    #[test]
    fn test_convert_mania4k() {
        assert_eq!(Ruleset::try_from(3), Ok(Ruleset::Mania4k));
    }

    #[test]
    fn test_convert_mania7k() {
        assert_eq!(Ruleset::try_from(4), Ok(Ruleset::Mania7k));
    }

    #[test]
    fn test_convert_invalid() {
        assert_eq!(Ruleset::try_from(5), Err(()));
    }

    #[test]
    fn test_enumerate() {
        let rulesets = Ruleset::iter().collect::<Vec<_>>();
        assert_eq!(
            rulesets,
            vec![
                Ruleset::Osu,
                Ruleset::Taiko,
                Ruleset::Catch,
                Ruleset::Mania4k,
                Ruleset::Mania7k
            ]
        );
    }
}
