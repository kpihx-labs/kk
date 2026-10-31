//! Bounded RTP reassembly before native VP8 decoder input. Compressed byte
//! limits alone do not bound the dimensions claimed by a tiny keyframe.
use super::*;
use std::{collections::BTreeMap, time::Instant};

struct Frame {
    created: Instant,
    start: Option<u16>,
    end: Option<u16>,
    encoded: usize,
    wire: usize,
    packets: BTreeMap<u16, Packet>,
}
#[derive(Default)]
pub(super) struct Guard {
    frames: BTreeMap<u32, Frame>,
    ssrc: Option<u32>,
    key_seen: bool,
}

// RFC 7741 optional descriptor fields; none are forwarded to the codec header
// validator. S only starts a whole frame for partition zero.
fn descriptor(packet: &Packet) -> Result<(bool, &[u8])> {
    let bytes = packet.payload();
    let first = *bytes.first().ok_or(Error::Protocol)?;
    if first & 0x40 != 0 || first & 0x0f > 8 {
        return Err(Error::Protocol);
    }
    let mut at = 1;
    if first & 0x80 != 0 {
        let extension = *bytes.get(at).ok_or(Error::Protocol)?;
        at += 1;
        if extension & 0x0f != 0 {
            return Err(Error::Protocol);
        }
        if extension & 0x80 != 0 {
            let picture = *bytes.get(at).ok_or(Error::Protocol)?;
            at += 1;
            if picture & 0x80 != 0 {
                at += 1;
            }
        }
        if extension & 0x40 != 0 {
            at += 1;
        }
        if extension & 0x30 != 0 {
            at += 1;
        }
    }
    let payload = bytes
        .get(at..)
        .filter(|payload| !payload.is_empty())
        .ok_or(Error::Protocol)?;
    Ok((first & 0x10 != 0 && first & 0x0f == 0, payload))
}

fn dimensions(head: &[u8]) -> bool {
    if head.len() < 10
        || head[0] & 1 != 0
        || (head[0] >> 1) & 7 > 3
        || head[3..6] != [0x9d, 1, 0x2a]
    {
        return false;
    }
    let width = u16::from_le_bytes([head[6], head[7]]) & 0x3fff;
    let height = u16::from_le_bytes([head[8], head[9]]) & 0x3fff;
    width > 0
        && height > 0
        && width <= config::MEDIA_VIDEO_WIDTH
        && height <= config::MEDIA_VIDEO_HEIGHT
}

impl Guard {
    pub(super) fn push(&mut self, packet: Packet) -> Result<Vec<Packet>> {
        if packet.codec() != Codec::Vp8 {
            return Err(Error::Protocol);
        }
        let (start, body) = descriptor(&packet)?;
        let encoded = body.len();
        let sequence = u16::from_be_bytes([packet.bytes()[2], packet.bytes()[3]]);
        let marker = packet.bytes()[1] & 0x80 != 0;
        if self.ssrc != Some(packet.ssrc()) {
            self.frames.clear();
            self.key_seen = false;
            self.ssrc = Some(packet.ssrc());
        }
        self.frames
            .retain(|_, frame| frame.created.elapsed() <= config::MEDIA_PACKET_MAX_AGE);
        let timestamp = packet.timestamp();
        if !self.frames.contains_key(&timestamp) && self.frames.len() == 2 {
            let oldest = *self
                .frames
                .iter()
                .min_by_key(|(_, frame)| frame.created)
                .unwrap()
                .0;
            self.frames.remove(&oldest);
        }
        let frame = self.frames.entry(timestamp).or_insert_with(|| Frame {
            created: Instant::now(),
            start: None,
            end: None,
            encoded: 0,
            wire: 0,
            packets: BTreeMap::new(),
        });
        if let Some(previous) = frame.packets.get(&sequence) {
            return if previous.bytes() == packet.bytes() {
                Ok(vec![])
            } else {
                Err(Error::Protocol)
            };
        }
        if (start && frame.start.is_some_and(|old| old != sequence))
            || (marker && frame.end.is_some_and(|old| old != sequence))
        {
            return Err(Error::Protocol);
        }
        if frame.encoded + encoded > config::MEDIA_ENCODED_FRAME_BYTES
            || frame.wire + packet.bytes().len() > config::MEDIA_ENCODED_FRAME_BYTES * 2
        {
            return Err(Error::Protocol);
        }
        frame.encoded += encoded;
        frame.wire += packet.bytes().len();
        if start {
            frame.start = Some(sequence);
        }
        if marker {
            frame.end = Some(sequence);
        }
        frame.packets.insert(sequence, packet);
        let (Some(first), Some(last)) = (frame.start, frame.end) else {
            return Ok(vec![]);
        };
        let count = last.wrapping_sub(first) as usize + 1;
        if count > config::MEDIA_ENCODED_FRAME_BYTES * 2 / 14 {
            return Err(Error::Protocol);
        }
        if count != frame.packets.len()
            || !(0..count).all(|offset| {
                frame
                    .packets
                    .contains_key(&first.wrapping_add(offset as u16))
            })
        {
            return Ok(vec![]);
        }
        let mut frame = self.frames.remove(&timestamp).unwrap();
        let mut head = Vec::with_capacity(10);
        for offset in 0..count {
            let (_, body) = descriptor(&frame.packets[&first.wrapping_add(offset as u16)])?;
            head.extend(body.iter().take(10 - head.len()));
            if head.len() == 10 {
                break;
            }
        }
        let key = head.first().is_some_and(|byte| byte & 1 == 0);
        if key {
            if !dimensions(&head) {
                self.key_seen = false;
                return Err(Error::Protocol);
            }
            self.key_seen = true;
        } else if !self.key_seen {
            return Ok(vec![]);
        }
        // Nothing reaches the native decoder until the complete frame and its
        // actual keyframe dimensions have passed the guard.
        Ok((0..count)
            .map(|offset| {
                frame
                    .packets
                    .remove(&first.wrapping_add(offset as u16))
                    .unwrap()
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(sequence: u16, timestamp: u32, start: bool, end: bool, payload: &[u8]) -> Packet {
        let mut bytes = vec![0x80, 97 | if end { 128 } else { 0 }];
        bytes.extend(sequence.to_be_bytes());
        bytes.extend(timestamp.to_be_bytes());
        bytes.extend(1u32.to_be_bytes());
        bytes.push(if start { 0x10 } else { 0 });
        bytes.extend(payload);
        Packet::parse(Codec::Vp8, bytes).unwrap()
    }
    fn key(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0, 0, 0, 0x9d, 1, 0x2a];
        bytes.extend(width.to_le_bytes());
        bytes.extend(height.to_le_bytes());
        bytes
    }
    #[test]
    fn tiny_oversized_and_malformed_native_keyframes_never_reach_decoder_input() {
        for bytes in [key(16383, 16383), key(0, 1), vec![0; 10], vec![0; 9]] {
            let mut guard = Guard::default();
            assert!(guard.push(packet(1, 1, true, true, &bytes)).is_err());
        }
    }
    #[test]
    fn header_split_reordering_sequence_wrap_and_interframes_preserve_valid_packets() {
        let mut guard = Guard::default();
        let bytes = key(config::MEDIA_VIDEO_WIDTH, config::MEDIA_VIDEO_HEIGHT);
        assert!(
            guard
                .push(packet(0, 1, false, true, &bytes[4..]))
                .unwrap()
                .is_empty()
        );
        let complete = guard
            .push(packet(65535, 1, true, false, &bytes[..4]))
            .unwrap();
        assert_eq!(complete.len(), 2);
        assert_eq!(&complete[0].bytes()[2..4], &65535u16.to_be_bytes());
        assert_eq!(
            guard
                .push(packet(1, 2, true, true, &[1, 0, 0]))
                .unwrap()
                .len(),
            1
        );
        let huge = key(16383, 16383);
        assert!(
            guard
                .push(packet(2, 3, true, false, &huge[..3]))
                .unwrap()
                .is_empty()
        );
        assert!(guard.push(packet(3, 3, false, true, &huge[3..])).is_err());
    }
}
