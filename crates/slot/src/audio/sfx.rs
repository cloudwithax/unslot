const INSERT: &[u8] = include_bytes!("../../assets/insert.pcm");
const EJECT: &[u8] = include_bytes!("../../assets/eject.pcm");
// The menu's sounds: recordings of real buttons and switches, credited in licenses/sounds.txt.
const MOVE: &[u8] = include_bytes!("../../assets/ui_move.pcm");
const OPEN: &[u8] = include_bytes!("../../assets/ui_open.pcm");
const BACK: &[u8] = include_bytes!("../../assets/ui_back.pcm");
const TOGGLE: &[u8] = include_bytes!("../../assets/ui_toggle.pcm");
const SHELF: &[u8] = include_bytes!("../../assets/ui_shelf.pcm");
const ERROR: &[u8] = include_bytes!("../../assets/ui_error.pcm");
const ASSET_HZ: f32 = 48_000.0;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Sfx {
    Insert,
    Eject,
    /// The cursor stepping to the next row, cart or key.
    Move,
    /// A page or menu opening, or A doing what it says.
    Open,
    /// Going back a page, or closing the menu.
    Back,
    /// A setting changed.
    Toggle,
    /// Another shelf.
    Shelf,
    /// Something refused.
    Error,
}

impl Sfx {
    pub const MENU: [Sfx; 6] = [
        Sfx::Move,
        Sfx::Open,
        Sfx::Back,
        Sfx::Toggle,
        Sfx::Shelf,
        Sfx::Error,
    ];

    /// How far into the clip its sound lands. The menu's clips start on theirs.
    pub fn lead(self) -> f32 {
        match self {
            Sfx::Insert => 0.097,
            Sfx::Eject => 0.021,
            _ => 0.0,
        }
    }

    /// The cart's own sounds, which the menu's never cut short.
    pub fn is_cart(self) -> bool {
        matches!(self, Sfx::Insert | Sfx::Eject)
    }

    fn pcm(self) -> &'static [u8] {
        match self {
            Sfx::Insert => INSERT,
            Sfx::Eject => EJECT,
            Sfx::Move => MOVE,
            Sfx::Open => OPEN,
            Sfx::Back => BACK,
            Sfx::Toggle => TOGGLE,
            Sfx::Shelf => SHELF,
            Sfx::Error => ERROR,
        }
    }

    pub fn tail(self) -> f32 {
        self.pcm().len() as f32 / 2.0 / ASSET_HZ - self.lead()
    }

    pub fn render(self, sample_rate: u32) -> Vec<i16> {
        let pcm = self.pcm();
        let mut out = Vec::with_capacity(pcm.len());
        for v in resampled(pcm, sample_rate) {
            let s = v.clamp(-32768.0, 32767.0) as i16;
            out.push(s);
            out.push(s);
        }
        out
    }
}

fn resampled(pcm: &[u8], sample_rate: u32) -> Vec<f32> {
    let src: Vec<f32> = pcm
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32)
        .collect();
    if sample_rate == ASSET_HZ as u32 || src.is_empty() {
        return src;
    }
    let ratio = sample_rate as f32 / ASSET_HZ;
    let n = (src.len() as f32 * ratio) as usize;
    (0..n)
        .map(|i| {
            let x = i as f32 / ratio;
            let a = x as usize;
            let f = x - a as f32;
            let b = (a + 1).min(src.len() - 1);
            src[a] * (1.0 - f) + src[b] * f
        })
        .collect()
}
