//! Die Bildformate, die im Chat erlaubt sind (echo-166), erkannt an den
//! ersten Bytes statt an der Dateiendung. Der Absender prueft vor dem
//! Hochladen, der Server nach dem Empfang: er dekodiert nie, soll aber keine
//! beliebigen Dateien als Bild an alle verteilen.

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
}

impl ImageFormat {
    /// So viele Bytes braucht [`ImageFormat::sniff`] hoechstens.
    pub const HEADER_BYTES: usize = 12;

    /// Das Format nach den ersten Bytes, oder `None`, wenn es keins der
    /// erlaubten ist.
    #[must_use]
    pub fn sniff(head: &[u8]) -> Option<Self> {
        if head.starts_with(PNG) {
            Some(Self::Png)
        } else if head.starts_with(JPEG) {
            Some(Self::Jpeg)
        } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
            Some(Self::Gif)
        } else if head.len() >= Self::HEADER_BYTES
            && head.starts_with(b"RIFF")
            && &head[8..12] == b"WEBP"
        {
            Some(Self::WebP)
        } else if head.starts_with(b"BM") {
            Some(Self::Bmp)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_allowed_formats() {
        assert_eq!(
            ImageFormat::sniff(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            ImageFormat::sniff(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(ImageFormat::sniff(b"GIF89a...."), Some(ImageFormat::Gif));
        assert_eq!(ImageFormat::sniff(b"GIF87a"), Some(ImageFormat::Gif));
        assert_eq!(
            ImageFormat::sniff(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageFormat::WebP)
        );
        assert_eq!(ImageFormat::sniff(b"BM\0\0"), Some(ImageFormat::Bmp));
    }

    #[test]
    fn refuses_everything_else() {
        assert_eq!(ImageFormat::sniff(b"PK\x03\x04"), None);
        assert_eq!(ImageFormat::sniff(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(ImageFormat::sniff(b"RIFF"), None);
        assert_eq!(ImageFormat::sniff(b""), None);
    }
}
