use slot_input::{Action, Btn};
use slot_retro::{ButtonMask, Sticks};

#[derive(Default)]
pub struct Pad {
    mask: u16,
}

impl Pad {
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::GbaDown(b) => {
                if let Some(bit) = bit(b) {
                    self.mask |= bit;
                }
            }
            Action::GbaUp(b) => {
                if let Some(bit) = bit(b) {
                    self.mask &= !bit;
                }
            }
            _ => {}
        }
    }

    pub fn mask(&self) -> ButtonMask {
        ButtonMask(self.mask)
    }

    pub fn clear(&mut self) {
        self.mask = 0;
    }
}

fn bit(btn: Btn) -> Option<u16> {
    Some(match btn {
        Btn::Up => ButtonMask::UP,
        Btn::Down => ButtonMask::DOWN,
        Btn::Left => ButtonMask::LEFT,
        Btn::Right => ButtonMask::RIGHT,
        Btn::A => ButtonMask::A,
        Btn::B => ButtonMask::B,
        Btn::X => ButtonMask::X,
        Btn::Y => ButtonMask::Y,
        Btn::L1 => ButtonMask::L,
        Btn::R1 => ButtonMask::R,
        Btn::L2 => ButtonMask::L2,
        Btn::R2 => ButtonMask::R2,
        Btn::L3 => ButtonMask::L3,
        Btn::R3 => ButtonMask::R3,
        Btn::Start => ButtonMask::START,
        Btn::Select => ButtonMask::SELECT,
        _ => return None,
    })
}

/// Moves the D-pad onto the left stick, for the models without one: a held direction pushes the
/// stick all the way, and the game sees the D-pad as let go.
pub fn dpad_as_stick(mask: ButtonMask, mut sticks: Sticks) -> (ButtonMask, Sticks) {
    let held = |bit| mask.0 & bit != 0;
    let axis = |less, more, at: i16| match (held(less), held(more)) {
        (true, false) => -i16::MAX,
        (false, true) => i16::MAX,
        _ => at,
    };
    sticks[0] = axis(ButtonMask::LEFT, ButtonMask::RIGHT, sticks[0]);
    sticks[1] = axis(ButtonMask::UP, ButtonMask::DOWN, sticks[1]);
    let dpad = ButtonMask::UP | ButtonMask::DOWN | ButtonMask::LEFT | ButtonMask::RIGHT;
    (ButtonMask(mask.0 & !dpad), sticks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dpad_pushes_the_left_stick_and_lets_go_of_itself() {
        let mask = ButtonMask(ButtonMask::UP | ButtonMask::RIGHT | ButtonMask::A);
        let (mask, sticks) = dpad_as_stick(mask, [0, 0, 5, 6]);
        assert_eq!(mask, ButtonMask(ButtonMask::A));
        assert_eq!(sticks, [i16::MAX, -i16::MAX, 5, 6]);
    }

    #[test]
    fn with_the_dpad_let_go_the_stick_reads_as_it_stands() {
        let (mask, sticks) = dpad_as_stick(ButtonMask(ButtonMask::B), [100, -200, 0, 0]);
        assert_eq!(mask, ButtonMask(ButtonMask::B));
        assert_eq!(sticks, [100, -200, 0, 0]);
    }
}
