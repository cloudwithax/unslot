//! The encoders: x264 for the picture and Opus or AAC for the sound, all in `libslotcast.so`,
//! which carries them, and the TLS the TV is talked to with, behind a few functions of its own
//! and is loaded only when a cast starts.

use std::ffi::{c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::Library;

type Ptr = *mut c_void;

/// The library's functions.
pub struct Codecs {
    _lib: Library,
    h264_open: unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_int) -> Ptr,
    h264_encode: unsafe extern "C" fn(Ptr, *const u8, c_int, *mut *const u8, *mut c_int) -> c_int,
    h264_close: unsafe extern "C" fn(Ptr),
    opus_open: unsafe extern "C" fn(c_int, c_int, c_int) -> Ptr,
    opus_encode: unsafe extern "C" fn(Ptr, *const i16, c_int, *mut u8, c_int) -> c_int,
    opus_close: unsafe extern "C" fn(Ptr),
    aac_open: unsafe extern "C" fn(c_int, c_int) -> Ptr,
    aac_encode: unsafe extern "C" fn(Ptr, *const i16, c_int, *mut u8, c_int) -> c_int,
    aac_close: unsafe extern "C" fn(Ptr),
    pub(super) tls_connect: unsafe extern "C" fn(c_int) -> Ptr,
    pub(super) tls_read: unsafe extern "C" fn(Ptr, *mut u8, c_int) -> c_int,
    pub(super) tls_write: unsafe extern "C" fn(Ptr, *const u8, c_int) -> c_int,
    pub(super) tls_pending: unsafe extern "C" fn(Ptr) -> c_int,
    pub(super) tls_close: unsafe extern "C" fn(Ptr),
}

/// Where the library is: `SLOT_CAST_LIB` if set, else beside the cores.
pub fn library(root: &Path) -> PathBuf {
    std::env::var_os("SLOT_CAST_LIB")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("System").join("libslotcast.so"))
}

impl Codecs {
    pub fn load(path: &Path) -> Result<Arc<Codecs>, String> {
        let lib = unsafe { Library::new(path) }.map_err(|e| format!("no encoder: {e}"))?;
        // SAFETY: each is named for the function it is typed as, as slotcast.c has them.
        unsafe {
            macro_rules! sym {
                ($name:literal) => {
                    *lib.get(concat!($name, "\0").as_bytes())
                        .map_err(|e| e.to_string())?
                };
            }
            Ok(Arc::new(Codecs {
                h264_open: sym!("slotcast_h264_open"),
                h264_encode: sym!("slotcast_h264_encode"),
                h264_close: sym!("slotcast_h264_close"),
                opus_open: sym!("slotcast_opus_open"),
                opus_encode: sym!("slotcast_opus_encode"),
                opus_close: sym!("slotcast_opus_close"),
                aac_open: sym!("slotcast_aac_open"),
                aac_encode: sym!("slotcast_aac_encode"),
                aac_close: sym!("slotcast_aac_close"),
                tls_connect: sym!("slotcast_tls_connect"),
                tls_read: sym!("slotcast_tls_read"),
                tls_write: sym!("slotcast_tls_write"),
                tls_pending: sym!("slotcast_tls_pending"),
                tls_close: sym!("slotcast_tls_close"),
                _lib: lib,
            }))
        }
    }
}

/// `w` by `h` grown to the TV's 16:9, as a TV stretches whatever it is sent to fill its screen.
pub fn widescreen(w: u32, h: u32) -> (u32, u32) {
    let even = |n: u32| n.div_ceil(2) * 2;
    if w * 9 < h * 16 {
        (even(h * 16 / 9), h)
    } else {
        (w, even(w * 9 / 16))
    }
}

/// A Cast stream kept within a 480-line encoder budget. A native square 720-line canvas
/// otherwise becomes 1280x720, which software x264 on H700 cannot encode at 60 fps.
pub fn cast_size(w: u32, h: u32) -> (u32, u32) {
    let native = widescreen(w, h);
    if native.0 <= 854 && native.1 <= 480 {
        native
    } else {
        fit(native, (854, 480))
    }
}

/// An H.264 encoder for `w` by `h` frames, each converted with `fill` then encoded, a 16:9
/// picture no larger than 480 lines, with the frame centred and its shape kept.
pub struct H264 {
    codecs: Arc<Codecs>,
    enc: Ptr,
    pub w: u32,
    pub h: u32,
    i420: Vec<u8>,
    out: (u32, u32),
    /// The cached scaling map and picture, when the input must be resized.
    scaled: Option<Scaled>,
}

// SAFETY: the encoder is only used by the thread that has it.
unsafe impl Send for H264 {}

impl H264 {
    /// `threads` encode slices of each frame at once, for it to be done sooner.
    pub fn open(
        codecs: &Arc<Codecs>,
        w: u32,
        h: u32,
        fps: u32,
        kbps: u32,
        threads: u32,
    ) -> Result<H264, String> {
        H264::open_at(codecs, w, h, cast_size(w, h), fps, kbps, threads)
    }

    /// As `open`, but `out` in size, the frame scaled to fit it, as large as it goes keeping
    /// its shape.
    pub fn open_at(
        codecs: &Arc<Codecs>,
        w: u32,
        h: u32,
        (ow, oh): (u32, u32),
        fps: u32,
        kbps: u32,
        threads: u32,
    ) -> Result<H264, String> {
        let (wi, hi, t) = (ow as c_int, oh as c_int, threads as c_int);
        // SAFETY: plain numbers.
        let enc = unsafe { (codecs.h264_open)(wi, hi, fps as c_int, kbps as c_int, t) };
        if enc.is_null() {
            return Err(format!("x264 would not open for {ow}x{oh}"));
        }
        // Black, which the frames are drawn over and the sides stay.
        let luma = (ow * oh) as usize;
        let mut i420 = vec![128; luma * 3 / 2];
        i420[..luma].fill(16);
        Ok(H264 {
            codecs: codecs.clone(),
            enc,
            w,
            h,
            i420,
            out: (ow, oh),
            scaled: Some(fit((w, h), (ow, oh)))
                .filter(|&s| s != (w, h))
                .map(|size| Scaled::new((w, h), size)),
        })
    }

    /// Takes `bgrx`, a frame at least `w` by `h` and `stride` pixels wide, as the next to encode.
    pub fn fill(&mut self, bgrx: &[u8], stride: u32) {
        match &mut self.scaled {
            Some(scaled) => {
                scaled.fill(bgrx, stride);
                bgrx_to_i420(
                    &scaled.pixels,
                    scaled.size.0,
                    scaled.size,
                    self.out,
                    &mut self.i420,
                );
            }
            None => bgrx_to_i420(bgrx, stride, (self.w, self.h), self.out, &mut self.i420),
        }
    }

    /// The frame last filled, encoded: its NAL units and whether it is a key frame. None when
    /// x264 holds it back or fails.
    pub fn encode(&mut self, key: bool) -> Option<(&[u8], bool)> {
        let mut out = std::ptr::null();
        let mut is_key = 0;
        // SAFETY: the planes are the size the encoder was opened for; what it gives back stays
        // until the next call, which needs `&mut self`.
        unsafe {
            let n = (self.codecs.h264_encode)(
                self.enc,
                self.i420.as_ptr(),
                c_int::from(key),
                &mut out,
                &mut is_key,
            );
            if n <= 0 || out.is_null() {
                return None;
            }
            Some((std::slice::from_raw_parts(out, n as usize), is_key != 0))
        }
    }
}

impl Drop for H264 {
    fn drop(&mut self) {
        // SAFETY: opened by this encoder, closed once.
        unsafe { (self.codecs.h264_close)(self.enc) };
    }
}

/// An Opus encoder for 48 kHz stereo, a packet each 20 ms.
pub struct Opus {
    codecs: Arc<Codecs>,
    enc: Ptr,
    out: Vec<u8>,
}

// SAFETY: as for H264.
unsafe impl Send for Opus {}

pub const OPUS_FRAMES: usize = 960;

impl Opus {
    pub fn open(codecs: &Arc<Codecs>, bps: u32) -> Result<Opus, String> {
        // SAFETY: plain numbers.
        let enc = unsafe { (codecs.opus_open)(48_000, 2, bps as c_int) };
        if enc.is_null() {
            return Err("Opus would not open".into());
        }
        Ok(Opus {
            codecs: codecs.clone(),
            enc,
            out: vec![0; 4000],
        })
    }

    /// One packet of `OPUS_FRAMES` stereo frames.
    pub fn encode(&mut self, pcm: &[i16]) -> Option<&[u8]> {
        assert_eq!(pcm.len(), OPUS_FRAMES * 2);
        // SAFETY: the samples and the room are as long as said.
        let n = unsafe {
            (self.codecs.opus_encode)(
                self.enc,
                pcm.as_ptr(),
                OPUS_FRAMES as c_int,
                self.out.as_mut_ptr(),
                self.out.len() as c_int,
            )
        };
        (n > 0).then(|| &self.out[..n as usize])
    }
}

impl Drop for Opus {
    fn drop(&mut self) {
        // SAFETY: opened by this encoder, closed once.
        unsafe { (self.codecs.opus_close)(self.enc) };
    }
}

/// An AAC-LC encoder for 48 kHz stereo, its frames in ADTS.
pub struct Aac {
    codecs: Arc<Codecs>,
    enc: Ptr,
    out: Vec<u8>,
}

// SAFETY: as for H264.
unsafe impl Send for Aac {}

pub const AAC_FRAMES: usize = 1024;

impl Aac {
    pub fn open(codecs: &Arc<Codecs>, bps: u32) -> Result<Aac, String> {
        // SAFETY: plain numbers.
        let enc = unsafe { (codecs.aac_open)(48_000, bps as c_int) };
        if enc.is_null() {
            return Err("AAC would not open".into());
        }
        Ok(Aac {
            codecs: codecs.clone(),
            enc,
            // As much as FAAC asks room for, whatever the frame.
            out: vec![0; 8192],
        })
    }

    /// Takes `AAC_FRAMES` stereo frames; gives back a frame of AAC, which the first two calls,
    /// while it fills, do not.
    pub fn encode(&mut self, pcm: &[i16]) -> Option<&[u8]> {
        assert_eq!(pcm.len(), AAC_FRAMES * 2);
        // SAFETY: the samples and the room are as long as said.
        let n = unsafe {
            (self.codecs.aac_encode)(
                self.enc,
                pcm.as_ptr(),
                AAC_FRAMES as c_int,
                self.out.as_mut_ptr(),
                self.out.len() as c_int,
            )
        };
        (n > 0).then(|| &self.out[..n as usize])
    }
}

impl Drop for Aac {
    fn drop(&mut self) {
        // SAFETY: opened by this encoder, closed once.
        unsafe { (self.codecs.aac_close)(self.enc) };
    }
}

/// `w` by `h` grown or shrunk to fit `ow` by `oh` keeping its shape, even both ways; as it is
/// when it fits already, as a narrower frame does a 16:9 picture as tall as it.
pub fn fit((w, h): (u32, u32), (ow, oh): (u32, u32)) -> (u32, u32) {
    if w <= ow && h == oh || w == ow && h <= oh {
        return (w, h);
    }
    let even = |n: u64| (n / 2 * 2) as u32;
    let (w, h, ow, oh) = (u64::from(w), u64::from(h), u64::from(ow), u64::from(oh));
    if w * oh > ow * h {
        (even(ow), even(h * ow / w))
    } else {
        (even(w * oh / h), even(oh))
    }
}

/// Nearest-neighbour scaling with its pixel map and output allocated once per encoder.
struct Scaled {
    size: (u32, u32),
    cols: Vec<usize>,
    rows: Vec<usize>,
    pixels: Vec<u8>,
}

impl Scaled {
    fn new((w, h): (u32, u32), (sw, sh): (u32, u32)) -> Self {
        Self {
            size: (sw, sh),
            cols: (0..u64::from(sw))
                .map(|x| (x * u64::from(w) / u64::from(sw)) as usize * 4)
                .collect(),
            rows: (0..u64::from(sh))
                .map(|y| (y * u64::from(h) / u64::from(sh)) as usize)
                .collect(),
            pixels: vec![0; (sw * sh * 4) as usize],
        }
    }

    fn fill(&mut self, bgrx: &[u8], stride: u32) {
        let (row_bytes, stride) = (self.size.0 as usize * 4, stride as usize * 4);
        let mut last = None;
        for (y, &from) in self.rows.iter().enumerate() {
            let (done, row) = self.pixels.split_at_mut(y * row_bytes);
            let row = &mut row[..row_bytes];
            if last == Some(from) {
                // The same row of the frame as the one above it.
                row.copy_from_slice(&done[(y - 1) * row_bytes..]);
                continue;
            }
            last = Some(from);
            let src = &bgrx[from * stride..];
            for (d, &c) in row.chunks_exact_mut(4).zip(&self.cols) {
                d.copy_from_slice(&src[c..c + 4]);
            }
        }
    }
}

/// The top-left `w` by `h` of `bgrx`, `stride` pixels to a row, into the middle of `out`, an I420
/// picture `ow` by `oh` with its planes one after another: BT.601's limited range, each chroma
/// sample the average of four pixels. AArch64 uses NEON on this thread; the scalar fallback
/// splits the top and bottom halves between two threads.
pub fn bgrx_to_i420(
    bgrx: &[u8],
    stride: u32,
    (w, h): (u32, u32),
    (ow, oh): (u32, u32),
    out: &mut [u8],
) {
    let (w, h, stride) = (w as usize, h as usize, stride as usize * 4);
    let (ow, oh) = (ow as usize, oh as usize);
    // Even, so each chroma sample is of four pixels of the frame.
    let (x0, y0) = ((ow - w) / 4 * 2, (oh - h) / 4 * 2);
    let (luma, chroma) = out.split_at_mut(ow * oh);
    let (u_plane, v_plane) = chroma.split_at_mut(ow * oh / 4);
    let luma = &mut luma[y0 * ow..(y0 + h) * ow];
    let (u_plane, v_plane) = (
        &mut u_plane[y0 / 2 * (ow / 2)..(y0 + h) / 2 * (ow / 2)],
        &mut v_plane[y0 / 2 * (ow / 2)..(y0 + h) / 2 * (ow / 2)],
    );
    let place = Place { stride, w, ow, x0 };
    #[cfg(target_arch = "aarch64")]
    rows_to_i420(bgrx, &place, luma, u_plane, v_plane);
    #[cfg(not(target_arch = "aarch64"))]
    {
        // Whole pairs of rows to each half.
        let top = h / 4 * 2;
        let (luma_top, luma_bottom) = luma.split_at_mut(top * ow);
        let (u_top, u_bottom) = u_plane.split_at_mut(top / 2 * (ow / 2));
        let (v_top, v_bottom) = v_plane.split_at_mut(top / 2 * (ow / 2));
        let below = &bgrx[(top * stride).min(bgrx.len())..];
        std::thread::scope(|s| {
            s.spawn(|| rows_to_i420(below, &place, luma_bottom, u_bottom, v_bottom));
            rows_to_i420(bgrx, &place, luma_top, u_top, v_top);
        });
    }
}

/// Where rows go: `w` pixels of rows `stride` bytes apart, to `x0` along rows `ow` wide.
struct Place {
    stride: usize,
    w: usize,
    ow: usize,
    x0: usize,
}

/// As many pairs of rows of `bgrx` as `luma` has room for.
fn rows_to_i420(bgrx: &[u8], at: &Place, luma: &mut [u8], u: &mut [u8], v: &mut [u8]) {
    let Place { stride, w, ow, x0 } = *at;
    let luma_rows = luma.chunks_exact_mut(ow * 2);
    let chroma_rows = u.chunks_exact_mut(ow / 2).zip(v.chunks_exact_mut(ow / 2));
    for (pair, (l, (u, v))) in luma_rows.zip(chroma_rows).enumerate() {
        let r0 = &bgrx[pair * 2 * stride..][..w * 4];
        let r1 = &bgrx[(pair * 2 + 1) * stride..][..w * 4];
        let (l0, l1) = l.split_at_mut(ow);
        let (l0, l1) = (&mut l0[x0..x0 + w], &mut l1[x0..x0 + w]);
        let (u, v) = (&mut u[x0 / 2..(x0 + w) / 2], &mut v[x0 / 2..(x0 + w) / 2]);
        // Eight pixels per row at a time; leave the even tail to the scalar conversion.
        // SAFETY: the rows have w luma samples, w*4 source bytes and w/2 chroma samples;
        // neon_rows reads and writes only complete eight-pixel blocks.
        #[cfg(target_arch = "aarch64")]
        let done = unsafe { neon_rows(r0, r1, l0, l1, u, v) };
        #[cfg(not(target_arch = "aarch64"))]
        let done = 0;
        let pixels = r0[done * 4..]
            .chunks_exact(8)
            .zip(r1[done * 4..].chunks_exact(8));
        let lumas = l0[done..]
            .chunks_exact_mut(2)
            .zip(l1[done..].chunks_exact_mut(2));
        for (((p0, p1), (y0, y1)), (u, v)) in pixels
            .zip(lumas)
            .zip(u[done / 2..].iter_mut().zip(&mut v[done / 2..]))
        {
            let px =
                |p: &[u8], i: usize| (i32::from(p[i + 2]), i32::from(p[i + 1]), i32::from(p[i]));
            let quad = [px(p0, 0), px(p0, 4), px(p1, 0), px(p1, 4)];
            let y_of =
                |(r, g, b): (i32, i32, i32)| (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
            y0[0] = y_of(quad[0]);
            y0[1] = y_of(quad[1]);
            y1[0] = y_of(quad[2]);
            y1[1] = y_of(quad[3]);
            let (r, g, b) = quad.iter().fold((0, 0, 0), |(r, g, b), &(pr, pg, pb)| {
                (r + pr, g + pg, b + pb)
            });
            *u = (((-38 * r - 74 * g + 112 * b + 512) >> 10) + 128) as u8;
            *v = (((112 * r - 94 * g - 18 * b + 512) >> 10) + 128) as u8;
        }
    }
}

/// The same integer BT.601 arithmetic as the scalar tail, including rounding before the
/// shifts. All slices cover a whole row, with four BGRX bytes per luma and one U/V per pair.
#[cfg(target_arch = "aarch64")]
unsafe fn neon_rows(
    r0: &[u8],
    r1: &[u8],
    y0: &mut [u8],
    y1: &mut [u8],
    u: &mut [u8],
    v: &mut [u8],
) -> usize {
    use std::arch::aarch64::*;
    let y = |p: uint8x8x4_t| {
        let sum = vmlal_u8(
            vmlal_u8(vmull_u8(p.2, vdup_n_u8(66)), p.1, vdup_n_u8(129)),
            p.0,
            vdup_n_u8(25),
        );
        vadd_u8(
            vshrn_n_u16::<8>(vaddq_u16(sum, vdupq_n_u16(128))),
            vdup_n_u8(16),
        )
    };
    let chroma = |sum: int32x4_t| {
        let val = vadd_s16(
            vshrn_n_s32::<10>(vaddq_s32(sum, vdupq_n_s32(512))),
            vdup_n_s16(128),
        );
        let bytes = vmovn_u16(vreinterpretq_u16_s16(vcombine_s16(val, vdup_n_s16(0))));
        vget_lane_u32::<0>(vreinterpret_u32_u8(bytes)).to_le_bytes()
    };
    let mut x = 0;
    while x + 8 <= y0.len() {
        let a = vld4_u8(r0.as_ptr().add(x * 4));
        let b = vld4_u8(r1.as_ptr().add(x * 4));
        vst1_u8(y0.as_mut_ptr().add(x), y(a));
        vst1_u8(y1.as_mut_ptr().add(x), y(b));
        let blue = vreinterpret_s16_u16(vadd_u16(vpaddl_u8(a.0), vpaddl_u8(b.0)));
        let green = vreinterpret_s16_u16(vadd_u16(vpaddl_u8(a.1), vpaddl_u8(b.1)));
        let red = vreinterpret_s16_u16(vadd_u16(vpaddl_u8(a.2), vpaddl_u8(b.2)));
        let us = vmlsl_n_s16(vmlsl_n_s16(vmull_n_s16(blue, 112), green, 74), red, 38);
        let vs = vmlsl_n_s16(vmlsl_n_s16(vmull_n_s16(red, 112), green, 94), blue, 18);
        u[x / 2..x / 2 + 4].copy_from_slice(&chroma(us));
        v[x / 2..x / 2 + 4].copy_from_slice(&chroma(vs));
        x += 8;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_matches_scalar_math_across_vector_tails_stride_and_padding() {
        let mut seed = 42u32;
        for w in [2, 6, 8, 10, 14, 16, 18, 30, 64] {
            for h in [2, 6, 10] {
                let (stride, ow, oh) = (w + 3, w + 6, h + 4);
                let mut src = vec![0u8; stride * h * 4];
                for byte in &mut src {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *byte = (seed >> 24) as u8;
                }
                let mut want = vec![128; ow * oh * 3 / 2];
                want[..ow * oh].fill(16);
                let mut got = want.clone();
                let px = |x: usize, y: usize| {
                    let i = (y * stride + x) * 4;
                    (
                        i32::from(src[i + 2]),
                        i32::from(src[i + 1]),
                        i32::from(src[i]),
                    )
                };
                let (dx, dy) = ((ow - w) / 4 * 2, (oh - h) / 4 * 2);
                for y in (0..h).step_by(2) {
                    for x in (0..w).step_by(2) {
                        let mut sum = (0, 0, 0);
                        for yy in 0..2 {
                            for xx in 0..2 {
                                let (r, g, b) = px(x + xx, y + yy);
                                want[(y + yy + dy) * ow + x + xx + dx] =
                                    (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
                                sum = (sum.0 + r, sum.1 + g, sum.2 + b);
                            }
                        }
                        let (r, g, b) = sum;
                        let i = (y + dy) / 2 * (ow / 2) + (x + dx) / 2;
                        want[ow * oh + i] =
                            (((-38 * r - 74 * g + 112 * b + 512) >> 10) + 128) as u8;
                        want[ow * oh * 5 / 4 + i] =
                            (((112 * r - 94 * g - 18 * b + 512) >> 10) + 128) as u8;
                    }
                }
                bgrx_to_i420(
                    &src,
                    stride as u32,
                    (w as u32, h as u32),
                    (ow as u32, oh as u32),
                    &mut got,
                );
                assert_eq!(got, want, "{w}x{h}");
            }
        }
    }

    #[test]
    fn large_canvases_stay_within_the_realtime_encoder_budget() {
        for input in [(720, 720), (1280, 720), (1920, 1080), (640, 480)] {
            let (w, h) = cast_size(input.0, input.1);
            assert!(w <= 854 && h <= 480);
            assert_eq!((w % 2, h % 2), (0, 0));
            // At most a pixel of rounding from 16:9.
            assert!(w.abs_diff(h * 16 / 9) <= 2);
        }
        // Don't upscale an already small canvas just to fill the encoder budget.
        assert_eq!(cast_size(320, 240), widescreen(320, 240));
    }

    #[test]
    fn colours_come_out_as_bt601_says() {
        // Two by two of white, then two by two of pure red, in a row three pixels wider.
        let stride = 7;
        let mut bgrx = vec![0u8; stride * 2 * 4];
        for y in 0..2 {
            for x in 0..4 {
                let px = if x < 2 {
                    [255, 255, 255, 0]
                } else {
                    [0, 0, 255, 0]
                };
                bgrx[(y * stride + x) * 4..][..4].copy_from_slice(&px);
            }
        }
        let mut out = vec![0u8; 4 * 2 * 3 / 2];
        bgrx_to_i420(&bgrx, stride as u32, (4, 2), (4, 2), &mut out);
        assert_eq!(&out[..8], &[235, 235, 82, 82, 235, 235, 82, 82]);
        assert_eq!(&out[8..10], &[128, 90], "U");
        assert_eq!(&out[10..12], &[128, 240], "V");
    }

    #[test]
    fn both_halves_are_converted() {
        // Six rows, two to the top half and four to the bottom: red, white, then red again.
        let (w, h) = (2usize, 6usize);
        let mut bgrx = vec![0u8; w * h * 4];
        for (y, row) in bgrx.chunks_exact_mut(w * 4).enumerate() {
            let px = if y / 2 == 1 {
                [255, 255, 255, 0]
            } else {
                [0, 0, 255, 0]
            };
            row.chunks_exact_mut(4).for_each(|p| p.copy_from_slice(&px));
        }
        let mut out = vec![0u8; w * h * 3 / 2];
        bgrx_to_i420(
            &bgrx,
            w as u32,
            (w as u32, h as u32),
            (w as u32, h as u32),
            &mut out,
        );
        assert_eq!(
            &out[..12],
            &[82, 82, 82, 82, 235, 235, 235, 235, 82, 82, 82, 82]
        );
        assert_eq!(&out[12..15], &[90, 128, 90], "U");
        assert_eq!(&out[15..18], &[240, 128, 240], "V");
    }

    #[test]
    fn a_frame_is_fitted_to_a_picture_keeping_its_shape() {
        assert_eq!(fit((720, 480), (1280, 720)), (1080, 720));
        assert_eq!(fit((720, 720), (1280, 720)), (720, 720));
        assert_eq!(fit((720, 480), (640, 480)), (640, 426));
        assert_eq!(fit((720, 480), (854, 480)), (720, 480));
    }

    #[test]
    fn each_scaled_pixel_is_the_nearest_of_the_frame() {
        // Two by two, a row wider, to three by three.
        let px = |v: u8| [v, v, v, 0];
        let bgrx = [px(1), px(2), px(9), px(3), px(4), px(9)].concat();
        let mut scaled = Scaled::new((2, 2), (3, 3));
        scaled.fill(&bgrx, 3);
        let got: Vec<u8> = scaled.pixels.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(got, [1, 1, 2, 1, 1, 2, 3, 3, 4]);
        // A second frame may have a different stride; only the pixel map is cached.
        let bgrx = [px(5), px(6), px(7), px(8)].concat();
        scaled.fill(&bgrx, 2);
        let got: Vec<u8> = scaled.pixels.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(got, [5, 5, 6, 5, 5, 6, 7, 7, 8]);
    }

    #[test]
    fn the_sp_is_shown_in_the_middle_of_a_widescreen_picture() {
        assert_eq!(widescreen(720, 480), (854, 480));
        assert_eq!(widescreen(1920, 1080), (1920, 1080));
        assert_eq!(widescreen(1000, 300), (1000, 562));
        // Two by two of white in the middle of eight by two of black.
        let bgrx = vec![255u8; 2 * 2 * 4];
        let mut out = vec![128u8; 8 * 2 * 3 / 2];
        out[..16].fill(16);
        bgrx_to_i420(&bgrx, 2, (2, 2), (8, 2), &mut out);
        let row = [16, 16, 235, 235, 16, 16, 16, 16];
        assert_eq!(&out[..16], [row, row].concat());
        assert_eq!(&out[16..20], &[128; 4], "U");
    }
}
