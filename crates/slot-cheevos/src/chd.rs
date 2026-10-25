//! A disc reader over CHD images for rcheevos, which only reads .cue/.bin and .iso itself.
//!
//! rcheevos asks for a track (by number, or "the first data track"), then for sectors by
//! absolute address, wanting each sector's user data (2048 bytes of a data track, 2352 of an
//! audio one) back to back. The shim hands it this reader for `.chd` paths and its own for
//! the rest.

use std::ffi::{c_char, c_void, CStr};
use std::fs::File;
use std::io::BufReader;

use chd::metadata::{KnownMetadata, MetadataTag};
use chd::Chd;

/// The bytes of one CD frame in a CHD: a 2352-byte sector, then 96 of subcode.
const FRAME: u64 = 2448;
const SECTOR: usize = 2352;
/// rcheevos' special track numbers (RC_HASH_CDTRACK_*).
const FIRST_DATA: u32 = u32::MAX;
const LAST: u32 = u32::MAX - 1;
const LARGEST: u32 = u32::MAX - 2;

#[derive(Clone, Debug, PartialEq)]
pub struct TrackInfo {
    pub number: u32,
    pub audio: bool,
    /// Bytes of each stored sector: 2352 raw, 2336 mode 2, 2048 cooked.
    pub sector_bytes: usize,
    /// The CHD frame holding the track's first sector after its pregap.
    pub first_frame: u64,
    /// Sectors from there to the end of the track.
    pub frames: u64,
    /// The disc address of that first sector.
    pub lba: u32,
}

/// The tracks of a CD, from the CHD's track metadata (`CHT2`, or the older `CHTR`).
pub fn tracks(entries: &[String]) -> Vec<TrackInfo> {
    let mut out = Vec::new();
    let mut frame = 0u64;
    let mut lba = 0u64;
    for entry in entries {
        let field = |name: &str| {
            entry
                .split_whitespace()
                .find_map(|kv| kv.strip_prefix(name)?.strip_prefix(':'))
                .unwrap_or("")
        };
        let number: u32 = field("TRACK").parse().unwrap_or(0);
        let kind = field("TYPE");
        let frames: u64 = field("FRAMES").parse().unwrap_or(0);
        let pregap: u64 = field("PREGAP").parse().unwrap_or(0);
        let postgap: u64 = field("POSTGAP").parse().unwrap_or(0);
        // A pregap type starting with V was ripped into the image, inside FRAMES.
        let stored = field("PGTYPE").starts_with('V');
        let sector_bytes = match kind {
            "MODE1" | "MODE2_FORM1" => 2048,
            "MODE2" | "MODE2_FORM_MIX" => 2336,
            "MODE2_FORM2" => 2324,
            _ => SECTOR,
        };
        let skip = if stored { pregap.min(frames) } else { 0 };
        if !stored {
            lba += pregap;
        }
        out.push(TrackInfo {
            number,
            audio: kind == "AUDIO",
            sector_bytes,
            first_frame: frame + skip,
            frames: frames - skip,
            lba: (lba + skip) as u32,
        });
        lba += frames + postgap;
        // Each track's frames are padded to a multiple of four in the image.
        frame += frames.div_ceil(4) * 4;
    }
    out.sort_by_key(|t| t.number);
    out
}

/// Which track rcheevos means by `track`.
pub fn pick(tracks: &[TrackInfo], track: u32) -> Option<&TrackInfo> {
    match track {
        FIRST_DATA => tracks.iter().find(|t| !t.audio),
        LAST => tracks.last(),
        // 0 is how rcheevos used to say "largest".
        LARGEST | 0 => tracks.iter().max_by_key(|t| t.frames),
        n => tracks.iter().find(|t| t.number == n),
    }
}

pub struct Track {
    chd: Chd<BufReader<File>>,
    info: TrackInfo,
    /// Where a sector's user data starts, and how much of it there is.
    data_offset: usize,
    data_size: usize,
    hunk_bytes: u64,
    hunk: Vec<u8>,
    compressed: Vec<u8>,
    loaded: Option<u32>,
}

impl Track {
    pub fn open(path: &str, track: u32) -> Option<Track> {
        let file = BufReader::new(File::open(path).ok()?);
        let mut chd = Chd::open(file, None).ok()?;
        let refs: Vec<_> = chd.metadata_refs().collect::<Result<_, _>>().ok()?;
        let mut entries = Vec::new();
        for r in refs {
            let tag = r.metatag();
            if tag == KnownMetadata::CdRomTrack2 as u32 || tag == KnownMetadata::CdRomTrack as u32 {
                let m = r.read(chd.inner()).ok()?;
                let text = String::from_utf8_lossy(&m.value);
                entries.push(text.trim_end_matches('\0').to_string());
            }
        }
        let info = pick(&tracks(&entries), track)?.clone();
        let hunk_bytes = chd.header().hunk_size() as u64;
        let hunk = chd.get_hunksized_buffer();
        let mut t = Track {
            chd,
            info,
            data_offset: 0,
            data_size: 0,
            hunk_bytes,
            hunk,
            compressed: Vec::new(),
            loaded: None,
        };
        (t.data_offset, t.data_size) = if t.info.audio {
            (0, SECTOR)
        } else {
            match t.info.sector_bytes {
                2048 => (0, 2048),
                2336 | 2324 => (8, 2048),
                // A raw sector: past its sync and header (and a mode 2 subheader).
                _ => {
                    let mut first = [0u8; SECTOR];
                    t.frame(t.info.first_frame, &mut first)?;
                    // The header's mode byte; chd rebuilds the sync and header it strips.
                    let mode2 = first[15] == 2;
                    (if mode2 { 24 } else { 16 }, 2048)
                }
            }
        };
        Some(t)
    }

    /// The stored sector of one CHD frame.
    fn frame(&mut self, frame: u64, out: &mut [u8; SECTOR]) -> Option<()> {
        let at = frame * FRAME;
        let hunk = (at / self.hunk_bytes) as u32;
        let within = (at % self.hunk_bytes) as usize;
        if self.loaded != Some(hunk) {
            self.loaded = None;
            let mut h = self.chd.hunk(hunk).ok()?;
            h.read_hunk_in(&mut self.compressed, &mut self.hunk).ok()?;
            self.loaded = Some(hunk);
        }
        out.copy_from_slice(self.hunk.get(within..within + SECTOR)?);
        if self.info.audio {
            // CHD keeps audio big-endian; a .bin has it little-endian.
            for pair in out.chunks_exact_mut(2) {
                pair.swap(0, 1);
            }
        }
        Some(())
    }

    pub fn first_sector(&self) -> u32 {
        self.info.lba
    }

    /// Reads up to `out.len()` bytes of user data from the absolute `sector` on.
    pub fn read(&mut self, sector: u32, out: &mut [u8]) -> usize {
        let Some(mut index) = sector.checked_sub(self.info.lba).map(u64::from) else {
            return 0;
        };
        let mut done = 0;
        let mut raw = [0u8; SECTOR];
        while done < out.len() && index < self.info.frames {
            if self
                .frame(self.info.first_frame + index, &mut raw)
                .is_none()
            {
                break;
            }
            let n = self.data_size.min(out.len() - done);
            out[done..done + n].copy_from_slice(&raw[self.data_offset..self.data_offset + n]);
            done += n;
            index += 1;
        }
        done
    }
}

#[no_mangle]
extern "C" fn slot_chd_open_track(path: *const c_char, track: u32) -> *mut c_void {
    let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
        return std::ptr::null_mut();
    };
    match Track::open(path, track) {
        Some(t) => Box::into_raw(Box::new(t)).cast(),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
extern "C" fn slot_chd_read_sector(
    handle: *mut c_void,
    sector: u32,
    buf: *mut c_void,
    len: usize,
) -> usize {
    if handle.is_null() || buf.is_null() {
        return 0;
    }
    let track = unsafe { &mut *handle.cast::<Track>() };
    let out = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), len) };
    track.read(sector, out)
}

#[no_mangle]
extern "C" fn slot_chd_first_track_sector(handle: *mut c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }
    unsafe { &*handle.cast::<Track>() }.first_sector()
}

#[no_mangle]
extern "C" fn slot_chd_close_track(handle: *mut c_void) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle.cast::<Track>()) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_follow_one_another_padded_to_four_frames() {
        let t = tracks(&[
            "TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:1001 PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0".into(),
            "TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:300 PREGAP:150 PGTYPE:VAUDIO PGSUB:RW POSTGAP:0".into(),
        ]);
        assert_eq!(t[0].first_frame, 0);
        assert_eq!(t[0].frames, 1001);
        assert_eq!(t[0].lba, 0);
        assert!(t[1].audio);
        // Track 1 takes 1004 frames of the image; track 2's stored pregap is skipped.
        assert_eq!(t[1].first_frame, 1004 + 150);
        assert_eq!(t[1].frames, 150);
        assert_eq!(t[1].lba, 1001 + 150);
        assert_eq!(pick(&t, FIRST_DATA).unwrap().number, 1);
        assert_eq!(pick(&t, LAST).unwrap().number, 2);
    }
}
