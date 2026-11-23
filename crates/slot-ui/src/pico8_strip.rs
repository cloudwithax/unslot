//! The strip under a PICO-8 cart's label, its name printed on it in PICO-8's own letters, as
//! the printed carts PICO-8's fans make carry it.

/// The dark strip under the label the cart's name is printed on: x, y, w, h in
/// `assets/pico8_neutral.png`'s pixels.
pub const PICO8_STRIP: (u32, u32, u32, u32) = (27, 242, 210, 56);

/// PICO-8's own 3x5 letters, each row three bits, the leftmost the highest. Lower case is
/// printed as upper case, as PICO-8 itself prints it.
fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        'A' => [7, 5, 7, 5, 5],
        'B' => [7, 5, 6, 5, 7],
        'C' => [7, 4, 4, 4, 7],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [7, 4, 5, 5, 7],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [7, 2, 2, 2, 6],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [7, 7, 5, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' | '0' => [7, 5, 5, 5, 7],
        'P' => [7, 5, 7, 4, 4],
        'Q' => [7, 5, 5, 6, 3],
        'R' => [7, 5, 6, 5, 5],
        'S' => [7, 4, 7, 1, 7],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 5, 7, 7],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 7, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '1' => [6, 2, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 3, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [4, 4, 7, 5, 7],
        '7' => [7, 1, 1, 1, 1],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 1],
        '-' => [0, 0, 7, 0, 0],
        '.' => [0, 0, 0, 0, 2],
        ',' => [0, 0, 0, 2, 4],
        ':' => [0, 2, 0, 2, 0],
        '!' => [2, 2, 2, 0, 2],
        '?' => [7, 1, 3, 0, 2],
        '\'' => [2, 2, 0, 0, 0],
        '&' => [6, 5, 6, 5, 7],
        '+' => [0, 2, 7, 2, 0],
        '/' => [1, 1, 2, 4, 4],
        '(' => [2, 4, 4, 4, 2],
        ')' => [2, 1, 1, 1, 2],
        _ => [0; 5],
    }
}

const STRIP_PX: i32 = 2;
const STRIP_PAD: i32 = 9;
const STRIP_LINE: i32 = 7 * STRIP_PX;
const STRIP_TITLE_INK: [u8; 3] = [0xff, 0xf1, 0xe8];
const STRIP_INK: [u8; 3] = [0xc2, 0xc3, 0xc7];
/// The last line on every strip, as on the printed carts.
const STRIP_FOOT: &str = "PICO-8 CARTRIDGE";

/// The lines printed on a cart's strip: its title, wrapped to two lines at most and cut short
/// past that, then [`STRIP_FOOT`].
pub fn strip_lines(title: &str) -> Vec<String> {
    let room = ((PICO8_STRIP.2 as i32 - 2 * STRIP_PAD + 1) / (4 * STRIP_PX)).max(1) as usize;
    let mut lines: Vec<String> = Vec::new();
    for word in title.split_whitespace() {
        let word: String = word.chars().take(room).collect();
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= room => {
                line.push(' ');
                line.push_str(&word);
            }
            _ => lines.push(word),
        }
    }
    if lines.len() > 2 {
        lines.truncate(2);
        let last = &mut lines[1];
        while last.chars().count() + 2 > room {
            last.pop();
        }
        last.push_str("..");
    }
    lines.push(STRIP_FOOT.to_string());
    lines
}

/// Prints `title` and the strip's last line on a PICO-8 cart's face (`w` wide), in PICO-8's
/// own letters, as the printed carts carry them.
pub fn print_strip(rgba: &mut [u8], w: u32, title: &str) {
    let (sx, sy, _, sh) = PICO8_STRIP;
    let lines = strip_lines(title);
    let block = lines.len() as i32 * STRIP_LINE - 2 * STRIP_PX;
    let mut y = sy as i32 + (sh as i32 - block) / 2;
    let last = lines.len() - 1;
    for (i, line) in lines.iter().enumerate() {
        let (ink, alpha) = match i == last {
            true => (STRIP_INK, 0xc0),
            false => (STRIP_TITLE_INK, 0xff),
        };
        print_pixels(
            rgba,
            w,
            line,
            sx as i32 + STRIP_PAD,
            y,
            STRIP_PX,
            ink,
            alpha,
        );
        y += STRIP_LINE;
    }
}

/// Prints `text` in PICO-8's letters onto `rgba` (`w` wide), each letter pixel `px` square,
/// from `(x, y)`, in `ink` at `alpha`.
#[allow(clippy::too_many_arguments)]
fn print_pixels(
    rgba: &mut [u8],
    w: u32,
    text: &str,
    x: i32,
    y: i32,
    px: i32,
    ink: [u8; 3],
    alpha: u8,
) {
    let h = (rgba.len() / 4) as u32 / w.max(1);
    let mut put = |cx: i32, cy: i32| {
        if cx < 0 || cy < 0 || cx >= w as i32 || cy >= h as i32 {
            return;
        }
        let at = ((cy as u32 * w + cx as u32) * 4) as usize;
        let a = alpha as u32;
        for k in 0..3 {
            rgba[at + k] = ((ink[k] as u32 * a + rgba[at + k] as u32 * (255 - a)) / 255) as u8;
        }
        rgba[at + 3] = rgba[at + 3].max(alpha);
    };
    for (i, c) in text.chars().enumerate() {
        let rows = glyph(c);
        for (r, bits) in rows.iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) == 0 {
                    continue;
                }
                let (gx, gy) = (x + (i as i32 * 4 + col) * px, y + r as i32 * px);
                for dy in 0..px {
                    for dx in 0..px {
                        put(gx + dx, gy + dy);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_title_wraps_to_two_lines_and_is_cut_short() {
        assert_eq!(strip_lines("Celeste"), ["Celeste", STRIP_FOOT]);
        let lines = strip_lines("the very long name of a cart nobody could fit on a strip");
        assert_eq!(lines.len(), 3);
        assert!(lines[1].ends_with(".."));
        assert_eq!(lines[2], STRIP_FOOT);
    }

    #[test]
    fn the_strip_is_printed_inside_itself() {
        let (w, h) = (264u32, 328u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        print_strip(&mut rgba, w, "jelpi");
        let (sx, sy, sw, sh) = PICO8_STRIP;
        let inked = |x: u32, y: u32| rgba[((y * w + x) * 4 + 3) as usize] > 0;
        assert!((0..h).any(|y| (0..w).any(|x| inked(x, y))));
        for y in 0..h {
            for x in 0..w {
                let inside = x >= sx && x < sx + sw && y >= sy && y < sy + sh;
                assert!(inside || !inked(x, y), "ink at {x},{y}");
            }
        }
    }
}
