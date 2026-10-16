#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Btn {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    Start,
    Select,
    Menu,
    VolUp,
    VolDown,
    Power,
    Lid,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RawEvent {
    Down(Btn),
    Up(Btn),
}

pub type Millis = u64;

pub trait InputSource {
    fn poll(&mut self, now: Millis) -> Vec<RawEvent>;

    fn menu_dpad_display(&self) -> bool {
        false
    }

    /// The analog sticks as they stand now: left x, left y, right x, right y, each -32767 to
    /// 32767 with left and up negative. A source with no sticks leaves them at rest.
    fn sticks(&self) -> [i16; 4] {
        [0; 4]
    }
}
