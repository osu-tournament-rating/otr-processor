use std::convert::TryFrom;


#[derive(Clone, Copy)]
pub enum Mode {
    Osu = 0,
    Taiko = 1,
    Catch = 2,
    Mania = 3
}

impl TryFrom<i32> for Mode {
    type Error = ();

    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            x if x == Mode::Osu as i32 => Ok(Mode::Osu),
            x if x == Mode::Taiko as i32 => Ok(Mode::Taiko),
            x if x == Mode::Catch as i32 => Ok(Mode::Catch),
            x if x == Mode::Mania as i32 => Ok(Mode::Mania),
            _ => Err(()),
        }
    }
}
