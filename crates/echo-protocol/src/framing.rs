//! Steuernachrichten auf dem Stream: 4 Byte Laenge (little endian), dann
//! die postcard-Bytes des [`Envelope`].

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Envelope, ProtocolError, Wire};

const LENGTH_BYTES: usize = 4;

/// Laengenpraefix und Inhalt in einem Puffer.
pub fn encode(envelope: &Envelope) -> Result<Vec<u8>, ProtocolError> {
    let mut frame = postcard::to_extend(envelope, vec![0; LENGTH_BYTES])?;
    let body_len = frame.len() - LENGTH_BYTES;
    if body_len > Wire::MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge(body_len));
    }
    // Passt: MAX_MESSAGE_BYTES liegt weit unter u32::MAX.
    let length = u32::try_from(body_len).map_err(|_| ProtocolError::TooLarge(body_len))?;
    frame[..LENGTH_BYTES].copy_from_slice(&length.to_le_bytes());
    Ok(frame)
}

/// Liest einen Inhalt ohne Laengenpraefix.
pub fn decode(body: &[u8]) -> Result<Envelope, ProtocolError> {
    Ok(postcard::from_bytes(body)?)
}

pub async fn write_message<W>(stream: &mut W, envelope: &Envelope) -> Result<(), ProtocolError>
where
    W: AsyncWrite + Unpin,
{
    stream.write_all(&encode(envelope)?).await?;
    stream.flush().await?;
    Ok(())
}

/// Liest genau eine Nachricht; `None` bei sauberem Ende des Streams.
pub async fn read_message<R>(stream: &mut R) -> Result<Option<Envelope>, ProtocolError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; LENGTH_BYTES];
    if !fill(stream, &mut header).await? {
        return Ok(None);
    }

    let length = u32::from_le_bytes(header);
    let body_len = usize::try_from(length).map_err(|_| ProtocolError::InvalidLength(length))?;
    if body_len == 0 || body_len > Wire::MAX_MESSAGE_BYTES {
        return Err(ProtocolError::InvalidLength(length));
    }

    let mut body = vec![0u8; body_len];
    if !fill(stream, &mut body).await? {
        return Err(ProtocolError::Truncated);
    }
    decode(&body).map(Some)
}

/// Fuellt `buffer` ganz; `false`, wenn der Stream vor dem ersten Byte endet.
async fn fill<R>(stream: &mut R, buffer: &mut [u8]) -> Result<bool, ProtocolError>
where
    R: AsyncRead + Unpin,
{
    let mut filled = 0;
    while filled < buffer.len() {
        let read = stream.read(&mut buffer[filled..]).await?;
        if read == 0 {
            return if filled == 0 {
                Ok(false)
            } else {
                Err(ProtocolError::Truncated)
            };
        }
        filled += read;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatScope, Message};

    fn chat(text: &str) -> Envelope {
        Envelope {
            request_id: 7,
            message: Message::SendChat {
                scope: ChatScope::Channel,
                target: 0,
                text: text.to_owned(),
            },
        }
    }

    #[tokio::test]
    async fn messages_round_trip_through_a_stream() {
        let mut wire = Vec::new();
        write_message(&mut wire, &chat("hallo")).await.unwrap();
        write_message(&mut wire, &Envelope::event(Message::Ping))
            .await
            .unwrap();

        let mut reader = wire.as_slice();
        assert_eq!(
            read_message(&mut reader).await.unwrap(),
            Some(chat("hallo"))
        );
        assert_eq!(
            read_message(&mut reader).await.unwrap(),
            Some(Envelope::event(Message::Ping))
        );
        assert_eq!(read_message(&mut reader).await.unwrap(), None);
    }

    #[tokio::test]
    async fn rejects_truncated_and_oversized_frames() {
        let frame = encode(&chat("hallo")).unwrap();
        let mut cut = &frame[..frame.len() - 1];
        assert!(matches!(
            read_message(&mut cut).await,
            Err(ProtocolError::Truncated)
        ));
        let mut half_header = &frame[..2];
        assert!(matches!(
            read_message(&mut half_header).await,
            Err(ProtocolError::Truncated)
        ));

        let huge = (u32::try_from(Wire::MAX_MESSAGE_BYTES).unwrap() + 1).to_le_bytes();
        assert!(matches!(
            read_message(&mut huge.as_slice()).await,
            Err(ProtocolError::InvalidLength(_))
        ));
        let empty = 0u32.to_le_bytes();
        assert!(matches!(
            read_message(&mut empty.as_slice()).await,
            Err(ProtocolError::InvalidLength(0))
        ));
    }

    #[test]
    fn refuses_to_encode_more_than_the_limit() {
        let text = "x".repeat(Wire::MAX_MESSAGE_BYTES);
        assert!(matches!(
            encode(&chat(&text)),
            Err(ProtocolError::TooLarge(_))
        ));
    }

    #[test]
    fn garbage_is_malformed() {
        assert!(matches!(
            decode(&[0xff; 8]),
            Err(ProtocolError::Malformed(_))
        ));
    }
}
