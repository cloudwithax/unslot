use gilrs::Button;
use slot_input::Btn;

pub fn pad_to_btn(button: Button) -> Option<Btn> {
    Some(match button {
        Button::DPadUp => Btn::Up,
        Button::DPadDown => Btn::Down,
        Button::DPadLeft => Btn::Left,
        Button::DPadRight => Btn::Right,
        Button::East => Btn::A,
        Button::South => Btn::B,
        Button::North => Btn::X,
        Button::West => Btn::Y,
        Button::LeftTrigger => Btn::L1,
        Button::RightTrigger => Btn::R1,
        Button::LeftTrigger2 => Btn::L2,
        Button::RightTrigger2 => Btn::R2,
        Button::Start => Btn::Start,
        Button::Select => Btn::Select,
        Button::Mode => Btn::Menu,
        Button::LeftThumb => Btn::L3,
        Button::RightThumb => Btn::R3,
        _ => return None,
    })
}
