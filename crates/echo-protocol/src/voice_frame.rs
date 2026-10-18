//! Ein Sprachframe: 20 ms Opus. Geht als QUIC-Datagramm ueber die
//! Steuerverbindung, sonst als eigener unidirektionaler Stream; so blockiert
//! ein verlorener Frame nur sich selbst. Gleiches Byte-Layout wie beim
//! C#-Vorgaenger:
//!
//! ```text
//! Byte 0      Art (1 = Sprache im Kanal, 2 = Musik in den Kanal)
//! Byte 1..2   Absender-ID (vom Server gesetzt; der Client schickt 0)
//! Byte 3..4   Sequenznummer des Absenders (Jitter-Puffer, Luecken)
//! Byte 5..8   Zeitstempel des Absenders in ms (Jitter-Messung)
//! Byte 9..    Opus-Nutzlast; leer = Ende des Sprechens
//! ```
//!
//! Alle Zahlen little endian.

use crate::ProtocolError;

const HEADER_SIZE: usize = 9;

/// Getrennt gehalten, damit Empfaenger Musik eigenstaendig mischen, regeln
/// und stummschalten koennen (echo-16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Voice = 1,
    Music = 2,
}

impl TryFrom<u8> for FrameKind {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Voice),
            2 => Ok(Self::Music),
            other => Err(ProtocolError::UnknownFrameKind(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceFrame<'a> {
    pub kind: FrameKind,
    pub sender: u16,
    pub sequence: u16,
    pub timestamp_ms: u32,
    pub opus: &'a [u8],
}

impl<'a> VoiceFrame<'a> {
    pub const HEADER_SIZE: usize = HEADER_SIZE;

    /// Ein leerer Frame beendet das Sprechen.
    #[must_use]
    pub fn is_end(&self) -> bool {
        self.opus.is_empty()
    }

    #[must_use]
    pub fn size(&self) -> usize {
        Self::HEADER_SIZE + self.opus.len()
    }

    /// Kopf und Nutzlast in einem Zug in einen neuen Puffer.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.size());
        bytes.push(self.kind as u8);
        bytes.extend_from_slice(&self.sender.to_le_bytes());
        bytes.extend_from_slice(&self.sequence.to_le_bytes());
        bytes.extend_from_slice(&self.timestamp_ms.to_le_bytes());
        bytes.extend_from_slice(self.opus);
        bytes
    }

    /// Liest einen Frame, ohne die Nutzlast zu kopieren.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ProtocolError> {
        let (header, opus) = bytes
            .split_first_chunk::<HEADER_SIZE>()
            .ok_or(ProtocolError::ShortVoiceFrame(bytes.len()))?;
        Ok(Self {
            kind: FrameKind::try_from(header[0])?,
            sender: u16::from_le_bytes([header[1], header[2]]),
            sequence: u16::from_le_bytes([header[3], header[4]]),
            timestamp_ms: u32::from_le_bytes([header[5], header[6], header[7], header[8]]),
            opus,
        })
    }

    /// Der Server setzt den Absender ein, bevor er den Frame weiterreicht;
    /// so kann kein Client sich als ein anderer ausgeben.
    pub fn stamp_sender(bytes: &mut [u8], sender: u16) -> Result<(), ProtocolError> {
        let len = bytes.len();
        let id = bytes
            .get_mut(1..3)
            .ok_or(ProtocolError::ShortVoiceFrame(len))?;
        id.copy_from_slice(&sender.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn layout_matches_the_csharp_version() {
        let frame = VoiceFrame {
            kind: FrameKind::Music,
            sender: 0x0102,
            sequence: 0x0304,
            timestamp_ms: 0x0506_0708,
            opus: &[0xaa, 0xbb],
        };
        assert_eq!(
            frame.to_bytes(),
            [
                2, 0x02, 0x01, 0x04, 0x03, 0x08, 0x07, 0x06, 0x05, 0xaa, 0xbb
            ]
        );
        assert_eq!(frame.size(), 11);
        assert!(!frame.is_end());
    }

    #[test]
    fn rejects_short_and_unknown_frames() {
        assert!(matches!(
            VoiceFrame::parse(&[1; 8]),
            Err(ProtocolError::ShortVoiceFrame(8))
        ));
        assert!(matches!(
            VoiceFrame::parse(&[9; 9]),
            Err(ProtocolError::UnknownFrameKind(9))
        ));
        assert!(VoiceFrame::parse(&[1; 9]).unwrap().is_end());
    }

    #[test]
    fn server_stamps_the_sender() {
        let mut bytes = VoiceFrame {
            kind: FrameKind::Voice,
            sender: 0,
            sequence: 1,
            timestamp_ms: 2,
            opus: b"opus",
        }
        .to_bytes();
        VoiceFrame::stamp_sender(&mut bytes, 42).unwrap();
        assert_eq!(VoiceFrame::parse(&bytes).unwrap().sender, 42);
        assert!(VoiceFrame::stamp_sender(&mut [1], 42).is_err());
    }

    proptest! {
        #[test]
        fn frames_round_trip(
            music: bool,
            sender: u16,
            sequence: u16,
            timestamp_ms: u32,
            opus in proptest::collection::vec(any::<u8>(), 0..400),
        ) {
            let kind = if music { FrameKind::Music } else { FrameKind::Voice };
            let frame = VoiceFrame { kind, sender, sequence, timestamp_ms, opus: &opus };
            let bytes = frame.to_bytes();
            prop_assert_eq!(VoiceFrame::parse(&bytes).unwrap(), frame);
        }

        #[test]
        fn parsing_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            let _ = VoiceFrame::parse(&bytes);
        }
    }
}
