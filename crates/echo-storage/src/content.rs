//! Ein Lesestrom ueber die Zeilen eines Inhalts im Archiv, der sich an jede
//! Stelle setzen laesst: Kino, Radio und Bildbetrachter springen darin
//! (echo-178). Liest Stueck fuer Stueck ueber inkrementelle Blobs, ohne eine
//! Zeile ganz zu laden.

use std::io::{self, Read, Seek, SeekFrom};

use crate::database::Connection;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Segment {
    pub(crate) rowid: i64,
    pub(crate) length: u64,
}

/// Haelt eine eigene Verbindung, bis er fallen gelassen wird.
pub struct ContentReader {
    connection: Connection,
    segments: Vec<Segment>,
    length: u64,
    position: u64,
    /// Die gerade offene Zeile und ihr Blob.
    open: Option<(usize, turso_core::Blob)>,
}

impl std::fmt::Debug for ContentReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentReader")
            .field("length", &self.length)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl ContentReader {
    pub(crate) fn new(connection: Connection, segments: Vec<Segment>) -> Self {
        let length = segments.iter().map(|s| s.length).sum();
        Self {
            connection,
            segments,
            length,
            position: 0,
            open: None,
        }
    }

    /// Die ganze Laenge des Inhalts.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Welche Zeile die Stelle traegt, und wo darin.
    fn locate(&self, position: u64) -> Option<(usize, u64)> {
        let mut start = 0;
        for (index, segment) in self.segments.iter().enumerate() {
            if position < start + segment.length {
                return Some((index, position - start));
            }
            start += segment.length;
        }
        None
    }

    fn blob(&mut self, index: usize) -> io::Result<&mut turso_core::Blob> {
        if self.open.as_ref().is_none_or(|(open, _)| *open != index) {
            self.close_blob();
            let rowid = self.segments[index].rowid;
            let blob = self
                .connection
                .blob("sqlar", "data", rowid, false)
                .map_err(io::Error::other)?;
            self.open = Some((index, blob));
        }
        match &mut self.open {
            Some((_, blob)) => Ok(blob),
            None => unreachable!("gerade geoeffnet"),
        }
    }

    fn close_blob(&mut self) {
        if let Some((_, blob)) = self.open.take() {
            // Nur gelesen: es gibt nichts festzuschreiben, was scheitern koennte.
            let _ = blob.close();
        }
    }
}

impl Read for ContentReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some((index, offset)) = self.locate(self.position) else {
            return Ok(0);
        };
        let available = self.segments[index].length - offset;
        let count = buf
            .len()
            .min(usize::try_from(available).unwrap_or(usize::MAX));
        // Eine Zeile ist hoechstens CHUNK_SIZE gross und passt in usize.
        let offset = usize::try_from(offset).map_err(io::Error::other)?;
        self.blob(index)?
            .read(offset, &mut buf[..count])
            .map_err(io::Error::other)?;
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for ContentReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(delta) => self.length.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        };
        let target = target.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "vor den Anfang gesprungen")
        })?;
        self.position = target;
        Ok(target)
    }
}

impl Drop for ContentReader {
    fn drop(&mut self) {
        self.close_blob();
    }
}
