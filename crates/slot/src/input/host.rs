use gilrs::{Axis, EventType, Gilrs};
use slot_input::{Btn, InputSource, Millis, RawEvent};
use winit::event::WindowEvent;
use winit::keyboard::{KeyCode, PhysicalKey};

use super::keys::key_to_btn;
use super::pad::pad_to_btn;

pub struct HostInput {
    pending: Vec<RawEvent>,
    pad: Option<Gilrs>,
    lid_closed: bool,
    held_keys: Vec<Btn>,
    sticks: [i16; 4],
}

impl HostInput {
    pub fn new() -> Self {
        HostInput {
            pending: Vec::new(),
            pad: Gilrs::new().ok(),
            lid_closed: false,
            held_keys: Vec::new(),
            sticks: [0; 4],
        }
    }

    pub fn on_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(false) => self.release_keys(),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                self.key(code, event.state.is_pressed(), event.repeat);
            }
            _ => {}
        }
    }

    fn release_keys(&mut self) {
        for btn in std::mem::take(&mut self.held_keys) {
            self.pending.push(RawEvent::Up(btn));
        }
    }

    pub fn key(&mut self, code: KeyCode, pressed: bool, repeat: bool) {
        if repeat {
            return;
        }
        let Some(btn) = key_to_btn(code) else {
            return;
        };
        if btn == Btn::Lid {
            if pressed {
                self.lid_closed = !self.lid_closed;
                self.pending.push(if self.lid_closed {
                    RawEvent::Down(Btn::Lid)
                } else {
                    RawEvent::Up(Btn::Lid)
                });
            }
            return;
        }
        match pressed {
            true if !self.held_keys.contains(&btn) => self.held_keys.push(btn),
            true => {}
            false => self.held_keys.retain(|b| *b != btn),
        }
        self.push(btn, pressed);
    }

    fn push(&mut self, btn: Btn, pressed: bool) {
        if pressed {
            crate::latency::read();
        }
        self.pending.push(if pressed {
            RawEvent::Down(btn)
        } else {
            RawEvent::Up(btn)
        });
    }

    fn drain_pad(&mut self) {
        let Some(pad) = self.pad.as_mut() else {
            return;
        };
        let mut edges = Vec::new();
        while let Some(ev) = pad.next_event() {
            match ev.event {
                EventType::ButtonPressed(b, _) => edges.push((b, true)),
                EventType::ButtonReleased(b, _) => edges.push((b, false)),
                EventType::AxisChanged(axis, value, _) => {
                    // gilrs counts up as positive; libretro counts it as negative.
                    let (at, sign) = match axis {
                        Axis::LeftStickX => (0, 1.0),
                        Axis::LeftStickY => (1, -1.0),
                        Axis::RightStickX => (2, 1.0),
                        Axis::RightStickY => (3, -1.0),
                        _ => continue,
                    };
                    self.sticks[at] = (value * sign * f32::from(i16::MAX)).round() as i16;
                }
                _ => {}
            }
        }
        for (b, pressed) in edges {
            if let Some(btn) = pad_to_btn(b) {
                self.push(btn, pressed);
            }
        }
    }
}

impl Default for HostInput {
    fn default() -> Self {
        HostInput::new()
    }
}

impl InputSource for HostInput {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.drain_pad();
        std::mem::take(&mut self.pending)
    }

    fn sticks(&self) -> [i16; 4] {
        self.sticks
    }
}
