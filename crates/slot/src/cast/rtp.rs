//! Cast Streaming's RTP and RTCP: each encoded frame encrypted with AES-128 in counter mode and
//! cut into packets under Cast's own header, kept a while for the receiver to ask again for any it
//! lost; the sender reports that tie a stream's clock to the wall's; and the receiver's feedback.

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aes::cipher::{KeyIvInit, StreamCipher};

type Ctr = ctr::Ctr128BE<aes::Aes128>;

/// The most of a frame one packet carries: with its headers, under a Wi-Fi network's 1500 bytes.
pub const MAX_PAYLOAD: usize = 1400;
/// How many of the last frames are kept to send again.
const KEEP: usize = 120;
/// Seconds from 1900, where NTP starts, to 1970.
const NTP_EPOCH: u64 = 2_208_988_800;

const RTCP_SR: u8 = 200;
const RTCP_PSFB: u8 = 206;
const PSFB_PLI: u8 = 1;
const PSFB_CAST: u8 = 15;

/// `data`, frame `frame_id` of a stream, encrypted or decrypted: the counter starts at the frame
/// id, big-endian at byte 8, under the stream's mask.
pub fn crypt(key: &[u8; 16], mask: &[u8; 16], frame_id: u32, data: &mut [u8]) {
    let mut iv = [0u8; 16];
    iv[8..12].copy_from_slice(&frame_id.to_be_bytes());
    for (i, m) in iv.iter_mut().zip(mask) {
        *i ^= m;
    }
    Ctr::new(key.into(), &iv.into()).apply_keystream(data);
}

/// One stream going out: its frames numbered from 0, their packets numbered across the stream.
pub struct Stream {
    pub ssrc: u32,
    payload_type: u8,
    key: [u8; 16],
    mask: [u8; 16],
    rate: u32,
    delay_ms: u16,
    seq: u16,
    next_frame: u32,
    sent: VecDeque<(u32, Vec<Vec<u8>>)>,
    packets: u32,
    octets: u32,
    /// The last frame's RTP time and when it was, for the sender reports.
    last: Option<(u32, Instant)>,
}

impl Stream {
    pub fn new(
        ssrc: u32,
        payload_type: u8,
        key: [u8; 16],
        mask: [u8; 16],
        rate: u32,
        delay_ms: u16,
    ) -> Stream {
        Stream {
            ssrc,
            payload_type,
            key,
            mask,
            rate,
            delay_ms,
            seq: 0,
            next_frame: 0,
            sent: VecDeque::new(),
            packets: 0,
            octets: 0,
            last: None,
        }
    }

    /// The packets for the next frame, `data` at RTP time `rtp`, which was `at`. A key frame
    /// stands alone; any other needs the one before.
    pub fn frame(&mut self, data: &[u8], rtp: u32, key: bool, at: Instant) -> &[Vec<u8>] {
        let id = self.next_frame;
        self.next_frame += 1;
        let mut body = data.to_vec();
        crypt(&self.key, &self.mask, id, &mut body);
        let chunks: Vec<&[u8]> = match body.is_empty() {
            true => vec![&[]],
            false => body.chunks(MAX_PAYLOAD).collect(),
        };
        let max = (chunks.len() - 1) as u16;
        let referenced = if key { id } else { id.wrapping_sub(1) };
        let packets = chunks
            .iter()
            .enumerate()
            .map(|(n, chunk)| {
                // The first packet of all says how long the receiver should hold frames for.
                let delay = id == 0 && n == 0;
                let mut p = Vec::with_capacity(23 + chunk.len());
                p.push(0x80);
                p.push(u8::from(n as u16 == max) << 7 | self.payload_type);
                p.extend_from_slice(&self.seq.to_be_bytes());
                p.extend_from_slice(&rtp.to_be_bytes());
                p.extend_from_slice(&self.ssrc.to_be_bytes());
                // Key frame, then a reference frame id follows, then how many extensions.
                p.push(u8::from(key) << 7 | 1 << 6 | u8::from(delay));
                p.push(id as u8);
                p.extend_from_slice(&(n as u16).to_be_bytes());
                p.extend_from_slice(&max.to_be_bytes());
                p.push(referenced as u8);
                if delay {
                    p.extend_from_slice(&(1u16 << 10 | 2).to_be_bytes());
                    p.extend_from_slice(&self.delay_ms.to_be_bytes());
                }
                p.extend_from_slice(chunk);
                self.seq = self.seq.wrapping_add(1);
                self.packets = self.packets.wrapping_add(1);
                self.octets = self.octets.wrapping_add(chunk.len() as u32);
                p
            })
            .collect();
        if self.sent.len() == KEEP {
            self.sent.pop_front();
        }
        self.sent.push_back((id, packets));
        self.last = Some((rtp, at));
        &self.sent.back().expect("just pushed").1
    }

    /// How many frames have gone.
    pub fn sent(&self) -> u32 {
        self.next_frame
    }

    /// The full frame id ending in `low`, the latest sent that does.
    pub fn expand(&self, low: u8) -> Option<u32> {
        let latest = self.next_frame.checked_sub(1)?;
        let back = latest.wrapping_sub(u32::from(low)) & 0xff;
        latest.checked_sub(back)
    }

    /// The packets of a frame the receiver says it lost: `packet` and those `more`'s bits pick
    /// out of the eight after it, or the whole frame when `packet` is 0xffff.
    pub fn lost(&self, frame: u8, packet: u16, more: u8) -> Vec<&[u8]> {
        let Some(id) = self.expand(frame) else {
            return Vec::new();
        };
        let Some((_, packets)) = self.sent.iter().find(|(f, _)| *f == id) else {
            return Vec::new();
        };
        if packet == 0xffff {
            return packets.iter().map(Vec::as_slice).collect();
        }
        let wanted = std::iter::once(packet)
            .chain((0..8).filter(|b| more >> b & 1 == 1).map(|b| packet + 1 + b));
        wanted
            .filter_map(|n| packets.get(n as usize))
            .map(Vec::as_slice)
            .collect()
    }

    /// A sender report for `now`, which is `wall` on the wall clock, once a frame has gone.
    pub fn sender_report(&self, now: Instant, wall: SystemTime) -> Option<Vec<u8>> {
        let (rtp, at) = self.last?;
        let since = now.saturating_duration_since(at).as_secs_f64();
        let rtp = rtp.wrapping_add((since * f64::from(self.rate)) as u32);
        let unix = wall.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
        let fraction = ((u64::from(unix.subsec_nanos()) << 32) / 1_000_000_000) as u32;
        let mut p = vec![0x80, RTCP_SR, 0, 6];
        p.extend_from_slice(&self.ssrc.to_be_bytes());
        p.extend_from_slice(&((unix.as_secs() + NTP_EPOCH) as u32).to_be_bytes());
        p.extend_from_slice(&fraction.to_be_bytes());
        p.extend_from_slice(&rtp.to_be_bytes());
        p.extend_from_slice(&self.packets.to_be_bytes());
        p.extend_from_slice(&self.octets.to_be_bytes());
        Some(p)
    }
}

/// What a receiver says back about one of the streams it gets, named by its SSRC.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Feedback {
    /// It cannot go on decoding until it has a key frame.
    KeyFrame { ssrc: u32 },
    /// Which frames it has all of, up to `checkpoint`, and which packets it is missing.
    Cast {
        ssrc: u32,
        checkpoint: u8,
        lost: Vec<Loss>,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Loss {
    pub frame: u8,
    pub packet: u16,
    pub more: u8,
}

/// The feedback in a compound RTCP packet; what is not feedback is passed over.
pub fn parse_rtcp(mut buf: &[u8]) -> Vec<Feedback> {
    let mut out = Vec::new();
    while buf.len() >= 4 {
        let format = buf[0] & 0x1f;
        let kind = buf[1];
        let len = 4 + 4 * usize::from(u16::from_be_bytes([buf[2], buf[3]]));
        let Some(body) = buf.get(4..len) else {
            break;
        };
        buf = &buf[len..];
        if kind != RTCP_PSFB || body.len() < 8 {
            continue;
        }
        let ssrc = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
        match format {
            PSFB_PLI => out.push(Feedback::KeyFrame { ssrc }),
            PSFB_CAST if body.len() >= 16 && &body[8..12] == b"CAST" => {
                let n = usize::from(body[13]);
                let lost = body[16..]
                    .chunks_exact(4)
                    .take(n)
                    .map(|l| Loss {
                        frame: l[0],
                        packet: u16::from_be_bytes([l[1], l[2]]),
                        more: l[3],
                    })
                    .collect();
                out.push(Feedback::Cast {
                    ssrc,
                    checkpoint: body[12],
                    lost,
                });
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream() -> Stream {
        Stream::new(77, 101, [1; 16], [2; 16], 90_000, 120)
    }

    #[test]
    fn a_frame_is_cut_into_packets_under_casts_header() {
        let mut s = stream();
        let t = Instant::now();
        let data = vec![9u8; MAX_PAYLOAD + 10];
        let packets = s.frame(&data, 3000, true, t).to_vec();
        assert_eq!(packets.len(), 2);
        let first = &packets[0];
        assert_eq!(first[0], 0x80);
        assert_eq!(first[1], 101, "the first of two is not the frame's last");
        assert_eq!(&first[4..8], &3000u32.to_be_bytes());
        assert_eq!(&first[8..12], &77u32.to_be_bytes());
        assert_eq!(first[12], 0x80 | 0x40 | 1, "key frame, reference id, one extension");
        assert_eq!(first[13], 0, "frame 0");
        assert_eq!(&first[14..18], &[0, 0, 0, 1], "packet 0 of 0..=1");
        assert_eq!(first[18], 0, "a key frame references itself");
        assert_eq!(&first[19..23], &[0x04, 0x02, 0, 120], "the playout delay");
        assert_eq!(first.len(), 23 + MAX_PAYLOAD);
        let last = &packets[1];
        assert_eq!(last[1], 0x80 | 101, "the frame's last packet is marked");
        assert_eq!(last[12], 0x80 | 0x40, "no extension after the first");
        assert_eq!(&last[2..4], &1u16.to_be_bytes(), "sequence numbers run on");
        assert_eq!(last.len(), 19 + 10);
        // The payload decrypts back to the frame.
        let mut body = [&first[23..], &last[19..]].concat();
        crypt(&[1; 16], &[2; 16], 0, &mut body);
        assert_eq!(body, data);

        let next = s.frame(&[5; 4], 6000, false, t).to_vec();
        assert_eq!((next[0][12], next[0][13], next[0][18]), (0x40, 1, 0));
        assert_eq!(next[0][1], 0x80 | 101);
    }

    #[test]
    fn frames_are_encrypted_with_their_own_counter() {
        let mut a = *b"the same frame..";
        let mut b = a;
        crypt(&[7; 16], &[0; 16], 0, &mut a);
        crypt(&[7; 16], &[0; 16], 1, &mut b);
        assert_ne!(a, b);
        assert_ne!(&a, b"the same frame..");
    }

    #[test]
    fn lost_packets_are_found_by_the_frames_low_byte() {
        let mut s = stream();
        let t = Instant::now();
        for i in 0..300u32 {
            s.frame(&vec![i as u8; MAX_PAYLOAD * 10], i * 3000, i == 0, t);
        }
        // Frame 299 ends in 43; packet 2, and the bits for 3 and 5.
        let lost = s.lost(43, 2, 0b101);
        assert_eq!(lost.len(), 3);
        assert_eq!(lost[0][13], 43);
        assert_eq!(
            lost.iter().map(|p| p[15]).collect::<Vec<_>>(),
            [2, 3, 5],
            "packet ids"
        );
        assert_eq!(s.lost(43, 0xffff, 0).len(), 10, "the whole frame");
        assert!(s.lost(100, 0, 0).is_empty(), "frame 100 is long gone");
    }

    #[test]
    fn a_sender_report_carries_the_wall_clock_and_the_streams_time_then() {
        let mut s = stream();
        let t = Instant::now();
        assert!(s.sender_report(t, SystemTime::now()).is_none());
        s.frame(&[1; 100], 9000, true, t);
        let wall = UNIX_EPOCH + Duration::from_millis(1_700_000_000_500);
        let r = s
            .sender_report(t + Duration::from_millis(100), wall)
            .unwrap();
        assert_eq!(&r[..4], &[0x80, 200, 0, 6]);
        assert_eq!(r.len(), 28);
        let word = |at: usize| u32::from_be_bytes(r[at..at + 4].try_into().unwrap());
        assert_eq!(word(4), 77);
        assert_eq!(u64::from(word(8)), 1_700_000_000 + NTP_EPOCH);
        assert_eq!(word(12), 1 << 31, "half a second");
        assert_eq!(word(16), 9000 + 9000, "a tenth of a second on");
        assert_eq!((word(20), word(24)), (1, 100));
    }

    #[test]
    fn feedback_is_read_out_of_a_compound_packet() {
        let mut p = vec![0x80, 201, 0, 1, 0, 0, 0, 9]; // a receiver report, passed over
        p.extend_from_slice(&[0x81, 206, 0, 2, 0, 0, 0, 9, 0, 0, 0, 77]); // PLI
        p.extend_from_slice(&[0x8f, 206, 0, 6, 0, 0, 0, 9, 0, 0, 0, 77]);
        p.extend_from_slice(b"CAST");
        p.extend_from_slice(&[12, 2, 0, 120, 13, 0, 4, 0b11, 14, 0xff, 0xff, 0]);
        assert_eq!(
            parse_rtcp(&p),
            [
                Feedback::KeyFrame { ssrc: 77 },
                Feedback::Cast {
                    ssrc: 77,
                    checkpoint: 12,
                    lost: vec![
                        Loss {
                            frame: 13,
                            packet: 4,
                            more: 0b11
                        },
                        Loss {
                            frame: 14,
                            packet: 0xffff,
                            more: 0
                        },
                    ],
                },
            ]
        );
    }
}
