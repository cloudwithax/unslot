//! Miracast's stream: the picture and the sound in one MPEG transport stream, as Wi-Fi Display
//! lays it out, up to seven of its packets to an RTP packet.

use std::time::Duration;

pub const VIDEO_PID: u16 = 0x1011;
pub const AUDIO_PID: u16 = 0x1100;
const PMT_PID: u16 = 0x100;
const PCR_PID: u16 = 0x1000;
const PACKET: usize = 188;
const PER_RTP: usize = 7;
/// RTP's payload type for an MPEG transport stream.
const MP2T: u8 = 33;
/// How often the tables and the clock go out.
const TABLES_EVERY: Duration = Duration::from_millis(100);

/// The sound's frames to an access unit: 10 ms of 48 kHz, in six of Wi-Fi Display's 80.
pub const LPCM_FRAMES: usize = 480;

/// How the sound goes: AAC, which a TV built on Android plays, as its parser takes LPCM's stream
/// type for AC-3, or LPCM, which every TV is meant to take.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sound {
    Aac,
    Lpcm,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Es {
    Video,
    Audio,
}

/// The stream's continuity counters and its RTP packets' numbering.
pub struct Mux {
    ssrc: u32,
    sound: Option<Sound>,
    seq: u16,
    pat_cc: u8,
    pmt_cc: u8,
    video_cc: u8,
    audio_cc: u8,
    tables: Option<Duration>,
}

impl Mux {
    pub fn new(ssrc: u32, sound: Option<Sound>) -> Mux {
        Mux {
            ssrc,
            sound,
            seq: 0,
            pat_cc: 0,
            pmt_cc: 0,
            video_cc: 0,
            audio_cc: 0,
            tables: None,
        }
    }

    /// One access unit of `es`, to be shown at `pts` from the stream's start, as RTP packets;
    /// `now` from the start. The tables and the clock go before it each tenth of a second.
    pub fn packets(&mut self, es: Es, unit: &[u8], pts: Duration, now: Duration) -> Vec<Vec<u8>> {
        let mut ts = Vec::with_capacity((unit.len() / 184 + 5) * PACKET);
        if self.tables.is_none_or(|t| now >= t + TABLES_EVERY) {
            self.tables = Some(now);
            self.pat(&mut ts);
            self.pmt(&mut ts);
            pcr(&mut ts, now);
        }
        self.pes(&mut ts, es, unit, pts);
        let time = (now.as_micros() * 9 / 100) as u32;
        ts.chunks(PACKET * PER_RTP)
            .map(|chunk| {
                let mut p = Vec::with_capacity(12 + chunk.len());
                p.extend_from_slice(&[0x80, MP2T]);
                p.extend_from_slice(&self.seq.to_be_bytes());
                p.extend_from_slice(&time.to_be_bytes());
                p.extend_from_slice(&self.ssrc.to_be_bytes());
                p.extend_from_slice(chunk);
                self.seq = self.seq.wrapping_add(1);
                p
            })
            .collect()
    }

    fn pat(&mut self, out: &mut Vec<u8>) {
        let pmt = [0xe0 | (PMT_PID >> 8) as u8, PMT_PID as u8];
        let section = [
            &[0x00, 0xb0, 0x0d, 0x00, 0x00, 0xc3, 0x00, 0x00, 0x00, 0x01][..],
            &pmt,
        ];
        table(out, 0, &mut self.pat_cc, &section.concat());
    }

    fn pmt(&mut self, out: &mut Vec<u8>) {
        let pid = |p: u16| [0xe0 | (p >> 8) as u8, p as u8];
        let mut s = vec![0x02, 0, 0, 0x00, 0x01, 0xc3, 0x00, 0x00];
        s.extend_from_slice(&pid(PCR_PID));
        s.extend_from_slice(&[0xf0, 0x00]);
        // H.264, no descriptors.
        s.push(0x1b);
        s.extend_from_slice(&pid(VIDEO_PID));
        s.extend_from_slice(&[0xf0, 0x00]);
        match self.sound {
            // AAC in ADTS, no descriptors.
            Some(Sound::Aac) => {
                s.push(0x0f);
                s.extend_from_slice(&pid(AUDIO_PID));
                s.extend_from_slice(&[0xf0, 0x00]);
            }
            // LPCM, with its descriptor: 48 kHz, 16 bits, stereo.
            Some(Sound::Lpcm) => {
                s.push(0x83);
                s.extend_from_slice(&pid(AUDIO_PID));
                s.extend_from_slice(&[
                    0xf0,
                    0x04,
                    0x83,
                    0x02,
                    (2 << 5) | (3 << 1),
                    (1 << 5) | 0x0f,
                ]);
            }
            None => {}
        }
        let len = s.len() - 3 + 4;
        s[1] = 0xb0 | (len >> 8) as u8;
        s[2] = len as u8;
        table(out, PMT_PID, &mut self.pmt_cc, &s);
    }

    fn pes(&mut self, out: &mut Vec<u8>, es: Es, unit: &[u8], pts: Duration) {
        let (pid, id, cc) = match es {
            Es::Video => (VIDEO_PID, 0xe0, &mut self.video_cc),
            Es::Audio if self.sound == Some(Sound::Aac) => (AUDIO_PID, 0xc0, &mut self.audio_cc),
            Es::Audio => (AUDIO_PID, 0xbd, &mut self.audio_cc),
        };
        let pts = (pts.as_micros() * 9 / 100) as u64 & ((1 << 33) - 1);
        // A video unit too long to say says 0, as it may.
        let len = u16::try_from(unit.len() + 8).unwrap_or(0);
        let mut payload = Vec::with_capacity(14 + unit.len());
        payload.extend_from_slice(&[0, 0, 1, id]);
        payload.extend_from_slice(&len.to_be_bytes());
        payload.extend_from_slice(&[0x84, 0x80, 0x05]);
        payload.extend_from_slice(&[
            0x20 | (((pts >> 30) & 7) << 1) as u8 | 1,
            (pts >> 22) as u8,
            (((pts >> 15) & 0x7f) << 1) as u8 | 1,
            (pts >> 7) as u8,
            ((pts & 0x7f) << 1) as u8 | 1,
        ]);
        payload.extend_from_slice(unit);
        packetize(out, pid, cc, &payload);
    }
}

/// `payload` cut into packets of `pid`, the first marked as its start, the last stuffed out to
/// size.
fn packetize(out: &mut Vec<u8>, pid: u16, cc: &mut u8, payload: &[u8]) {
    for (i, chunk) in payload.chunks(PACKET - 4).enumerate() {
        let start = if i == 0 { 0x40 } else { 0x00 };
        let stuffing = PACKET - 4 - chunk.len();
        let field = if stuffing > 0 { 0x30 } else { 0x10 };
        out.extend_from_slice(&[0x47, start | (pid >> 8) as u8, pid as u8, field | *cc]);
        *cc = (*cc + 1) & 0x0f;
        if stuffing > 0 {
            out.push(stuffing as u8 - 1);
            if stuffing > 1 {
                out.push(0x00);
                out.resize(out.len() + stuffing - 2, 0xff);
            }
        }
        out.extend_from_slice(chunk);
    }
}

/// A table section in a packet of its own, its CRC after it.
fn table(out: &mut Vec<u8>, pid: u16, cc: &mut u8, section: &[u8]) {
    let start = out.len();
    out.extend_from_slice(&[0x47, 0x40 | (pid >> 8) as u8, pid as u8, 0x10 | *cc, 0x00]);
    *cc = (*cc + 1) & 0x0f;
    out.extend_from_slice(section);
    out.extend_from_slice(&crc32(section).to_be_bytes());
    out.resize(start + PACKET, 0xff);
}

/// The program clock at `now`, in a packet of its own.
fn pcr(out: &mut Vec<u8>, now: Duration) {
    let pcr = now.as_nanos() as u64 * 27 / 1000;
    let (base, ext) = (pcr / 300, pcr % 300);
    let start = out.len();
    out.extend_from_slice(&[
        0x47,
        0x40 | (PCR_PID >> 8) as u8,
        PCR_PID as u8,
        0x20,
        0xb7,
        0x10,
        (base >> 25) as u8,
        (base >> 17) as u8,
        (base >> 9) as u8,
        (base >> 1) as u8,
        ((base & 1) << 7) as u8 | 0x7e | ((ext >> 8) & 1) as u8,
        ext as u8,
    ]);
    out.resize(start + PACKET, 0xff);
}

/// MPEG-2's CRC-32: most significant bit first, nothing reflected or inverted after.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// `LPCM_FRAMES` stereo frames as an LPCM access unit: Wi-Fi Display's four-byte header, then
/// the samples big-endian.
pub fn lpcm(pcm: &[i16]) -> Vec<u8> {
    assert_eq!(pcm.len(), LPCM_FRAMES * 2);
    // Six frame headers; 16 bits, 48 kHz, stereo.
    let mut unit = vec![0xa0, 6, 0, (2 << 3) | 1];
    unit.extend(pcm.iter().flat_map(|s| s.to_be_bytes()));
    unit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts_of(rtp: &[Vec<u8>]) -> Vec<u8> {
        rtp.iter().flat_map(|p| p[12..].to_vec()).collect()
    }

    fn pid(p: &[u8]) -> u16 {
        u16::from(p[1] & 0x1f) << 8 | u16::from(p[2])
    }

    #[test]
    fn the_crc_is_mpeg_2s() {
        // The CRC of "123456789" under CRC-32/MPEG-2.
        assert_eq!(crc32(b"123456789"), 0x0376_e6e7);
    }

    #[test]
    fn the_first_unit_brings_the_tables_and_the_clock_and_later_ones_do_not_for_a_while() {
        let mut mux = Mux::new(7, Some(Sound::Lpcm));
        let first = ts_of(&mux.packets(Es::Video, &[1; 10], Duration::ZERO, Duration::ZERO));
        let pids: Vec<u16> = first.chunks(PACKET).map(pid).collect();
        assert_eq!(pids, [0, PMT_PID, PCR_PID, VIDEO_PID]);
        let soon = Duration::from_millis(50);
        let next = ts_of(&mux.packets(Es::Audio, &[2; 10], soon, soon));
        assert_eq!(
            next.chunks(PACKET).map(pid).collect::<Vec<_>>(),
            [AUDIO_PID]
        );
        let later = Duration::from_millis(100);
        let again = ts_of(&mux.packets(Es::Video, &[3; 10], later, later));
        assert_eq!(again.chunks(PACKET).count(), 4);
    }

    #[test]
    fn the_tables_check_out() {
        let mut mux = Mux::new(7, Some(Sound::Lpcm));
        let ts = ts_of(&mux.packets(Es::Video, &[], Duration::ZERO, Duration::ZERO));
        for p in ts.chunks(PACKET).take(2) {
            // After the pointer field: the section, its length from byte 1, then its CRC.
            let s = &p[5..];
            let len = (usize::from(s[1] & 0x0f) << 8) | usize::from(s[2]);
            let (body, crc) = s[..3 + len].split_at(3 + len - 4);
            assert_eq!(crc32(body).to_be_bytes(), crc);
        }
        let pmt = &ts[PACKET + 5..];
        assert_eq!(pid(&[0, pmt[8], pmt[9]]), PCR_PID);
        assert_eq!((pmt[12], pid(&[0, pmt[13], pmt[14]])), (0x1b, VIDEO_PID));
        assert_eq!((pmt[17], pid(&[0, pmt[18], pmt[19]])), (0x83, AUDIO_PID));
        let mut aac = Mux::new(7, Some(Sound::Aac));
        let ts = ts_of(&aac.packets(Es::Audio, &[], Duration::ZERO, Duration::ZERO));
        let pmt = &ts[PACKET + 5..];
        assert_eq!((pmt[17], pid(&[0, pmt[18], pmt[19]])), (0x0f, AUDIO_PID));
        // An empty unit's PES header, at the end of its packet.
        assert_eq!(&ts[4 * PACKET - 14..][..4], &[0, 0, 1, 0xc0]);
    }

    #[test]
    fn a_unit_spans_packets_counted_in_turn_and_comes_back_whole() {
        let mut mux = Mux::new(7, Some(Sound::Lpcm));
        let unit: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
        let pts = Duration::from_millis(1000);
        let rtp = mux.packets(Es::Video, &unit, pts, Duration::ZERO);
        let ts = ts_of(&rtp);
        assert!(rtp
            .iter()
            .all(|p| p.len() <= 12 + PACKET * PER_RTP && p[1] == MP2T));
        assert_eq!(u16::from_be_bytes([rtp[1][2], rtp[1][3]]), 1);
        let video: Vec<&[u8]> = ts.chunks(PACKET).filter(|p| pid(p) == VIDEO_PID).collect();
        assert!(ts.chunks(PACKET).all(|p| p.len() == PACKET && p[0] == 0x47));
        let mut pes = Vec::new();
        for (i, p) in video.iter().enumerate() {
            assert_eq!(p[3] & 0x0f, i as u8, "continuity");
            assert_eq!(p[1] & 0x40 != 0, i == 0, "start");
            let skip = if p[3] & 0x20 != 0 {
                5 + usize::from(p[4])
            } else {
                4
            };
            pes.extend_from_slice(&p[skip..]);
        }
        assert_eq!(&pes[..4], &[0, 0, 1, 0xe0]);
        assert_eq!(u16::from_be_bytes([pes[4], pes[5]]), 1008);
        // 90 000 ticks, in the PTS's five bytes.
        let b = &pes[9..14];
        let got = (u64::from(b[0] >> 1) & 7) << 30
            | u64::from(b[1]) << 22
            | u64::from(b[2] >> 1) << 15
            | u64::from(b[3]) << 7
            | u64::from(b[4] >> 1);
        assert_eq!(got, 90_000);
        assert_eq!(&pes[14..], &unit[..]);
    }

    #[test]
    fn sound_goes_big_endian_under_its_header() {
        let mut pcm = vec![0i16; LPCM_FRAMES * 2];
        pcm[0] = 0x0102;
        pcm[1] = -2;
        let unit = lpcm(&pcm);
        assert_eq!(&unit[..8], &[0xa0, 6, 0, 0x11, 0x01, 0x02, 0xff, 0xfe]);
        assert_eq!(unit.len(), 4 + LPCM_FRAMES * 4);
    }
}
