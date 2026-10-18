//! Ein angenommener Client, wie der Rest des Servers ihn sieht: Kennung,
//! Name, Rolle, Kanal, und wie man ihm etwas schickt. Steuernachrichten
//! gehen ueber eine Sendequeue mit genau einem Schreiber; Sprache als
//! QUIC-Datagramm, sonst als eigener unidirektionaler Stream je Frame.
//!
//! Was sich aendert, aendert `ServerState` unter seiner Sperre. Kanal und
//! Stummschaltung liegen zusaetzlich in Atomics, weil der Sprachweg sie fuer
//! jedes Paket ohne Sperre liest.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use echo_protocol::{ClientDto, Envelope, LeaveReason, Message, Role, encode};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Haengt der Client, stapeln sich keine Frames: darueber wird verworfen.
const MAX_VOICE_IN_FLIGHT: usize = 64;

/// So lange darf die Sendequeue noch leeren, bevor ein geworfener Client
/// getrennt wird; sonst kaeme `Kicked` nicht mehr an.
const CLOSE_DELAY: Duration = Duration::from_millis(250);

/// Talk Power der Admins; Clients zeigen damit, wer sprechen darf.
const ADMIN_TALK_POWER: i32 = 100;

/// Ein kodierter Rahmen der Sendequeue; wer an viele schickt, kodiert nur
/// einmal (echo-80).
pub(crate) type Frame = Arc<[u8]>;

/// Was die Verbindung dem Client mitgibt.
#[derive(Debug, Clone)]
pub(crate) struct ClientLink {
    pub(crate) connection: quinn::Connection,
    pub(crate) outbox: mpsc::Sender<Frame>,
    pub(crate) closing: CancellationToken,
}

#[derive(Debug)]
struct Info {
    name: String,
    role: Role,
    away_message: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Client {
    id: u16,
    fingerprint: String,
    link: ClientLink,
    info: Mutex<Info>,
    channel: AtomicU64,
    input_muted: AtomicBool,
    output_muted: AtomicBool,
    leave: Mutex<(LeaveReason, Option<String>)>,
    voice_in_flight: Arc<AtomicUsize>,
}

/// Wie ein Client anfaengt.
#[derive(Debug)]
pub(crate) struct NewClient {
    pub(crate) id: u16,
    pub(crate) fingerprint: String,
    pub(crate) name: String,
    pub(crate) role: Role,
    pub(crate) channel: u64,
    pub(crate) input_muted: bool,
    pub(crate) output_muted: bool,
}

impl Client {
    #[must_use]
    pub(crate) fn new(new: NewClient, link: ClientLink) -> Self {
        Self {
            id: new.id,
            fingerprint: new.fingerprint,
            link,
            info: Mutex::new(Info {
                name: new.name,
                role: new.role,
                away_message: None,
            }),
            channel: AtomicU64::new(new.channel),
            input_muted: AtomicBool::new(new.input_muted),
            output_muted: AtomicBool::new(new.output_muted),
            leave: Mutex::new((LeaveReason::Left, None)),
            voice_in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[must_use]
    pub(crate) fn id(&self) -> u16 {
        self.id
    }

    #[must_use]
    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    #[must_use]
    pub(crate) fn name(&self) -> String {
        self.info.lock().name.clone()
    }

    pub(crate) fn set_name(&self, name: String) {
        self.info.lock().name = name;
    }

    #[must_use]
    pub(crate) fn role(&self) -> Role {
        self.info.lock().role
    }

    pub(crate) fn set_role(&self, role: Role) {
        self.info.lock().role = role;
    }

    pub(crate) fn set_away(&self, message: Option<String>) {
        self.info.lock().away_message = message;
    }

    #[must_use]
    pub(crate) fn channel(&self) -> u64 {
        self.channel.load(Ordering::Acquire)
    }

    pub(crate) fn set_channel(&self, channel: u64) {
        self.channel.store(channel, Ordering::Release);
    }

    #[must_use]
    pub(crate) fn input_muted(&self) -> bool {
        self.input_muted.load(Ordering::Relaxed)
    }

    pub(crate) fn set_input_muted(&self, muted: bool) {
        self.input_muted.store(muted, Ordering::Relaxed);
    }

    #[must_use]
    pub(crate) fn output_muted(&self) -> bool {
        self.output_muted.load(Ordering::Relaxed)
    }

    pub(crate) fn set_output_muted(&self, muted: bool) {
        self.output_muted.store(muted, Ordering::Relaxed);
    }

    #[must_use]
    pub(crate) fn to_dto(&self) -> ClientDto {
        let info = self.info.lock();
        ClientDto {
            id: self.id,
            channel: self.channel(),
            name: info.name.clone(),
            fingerprint: self.fingerprint.clone(),
            role: info.role,
            input_muted: self.input_muted(),
            output_muted: self.output_muted(),
            away_message: info.away_message.clone(),
            talk_power: if info.role >= Role::Admin {
                ADMIN_TALK_POWER
            } else {
                0
            },
        }
    }

    // --- Senden ---------------------------------------------------------------

    /// Ein Ereignis ohne Anfrage.
    pub(crate) fn send(&self, message: Message) {
        self.send_envelope(&Envelope::event(message));
    }

    /// Eine Antwort auf die Anfrage `request_id`.
    pub(crate) fn reply(&self, request_id: u32, message: Message) {
        self.send_envelope(&Envelope {
            request_id,
            message,
        });
    }

    fn send_envelope(&self, envelope: &Envelope) {
        // Was sich nicht kodieren laesst (zu gross), kann auch nicht ankommen.
        if let Ok(frame) = encode(envelope) {
            self.send_frame(Frame::from(frame));
        }
    }

    /// Ist die Queue voll, haengt der Client: dann bekommt er nichts mehr und
    /// wird getrennt, statt dass der Server fuer ihn Speicher sammelt.
    pub(crate) fn send_frame(&self, frame: Frame) {
        if let Err(mpsc::error::TrySendError::Full(_)) = self.link.outbox.try_send(frame) {
            self.close_later(LeaveReason::Timeout, "kommt nicht hinterher".into());
        }
    }

    /// Als Datagramm, wenn die Verbindung das kann und der Frame passt; sonst
    /// ein Frame = ein unidirektionaler Stream.
    pub(crate) fn send_voice(&self, frame: Bytes) {
        match self.link.connection.send_datagram(frame.clone()) {
            Ok(()) | Err(quinn::SendDatagramError::ConnectionLost(_)) => {}
            Err(_) => self.send_voice_as_stream(frame),
        }
    }

    fn send_voice_as_stream(&self, frame: Bytes) {
        if self.voice_in_flight.fetch_add(1, Ordering::AcqRel) >= MAX_VOICE_IN_FLIGHT {
            self.voice_in_flight.fetch_sub(1, Ordering::AcqRel);
            return;
        }
        let connection = self.link.connection.clone();
        let in_flight = Arc::clone(&self.voice_in_flight);
        tokio::spawn(async move {
            // Verbindung weg oder ueberlastet: der Frame ist verloren, mehr nicht.
            if let Ok(mut stream) = connection.open_uni().await {
                let _ = stream.write_all(&frame).await;
                let _ = stream.finish();
            }
            in_flight.fetch_sub(1, Ordering::AcqRel);
        });
    }

    // --- Ende -----------------------------------------------------------------

    /// Trennt nach kurzer Zeit, damit die Sendequeue noch leert (`Kicked`).
    pub(crate) fn close_later(&self, reason: LeaveReason, detail: String) {
        *self.leave.lock() = (reason, Some(detail));
        let closing = self.link.closing.clone();
        tokio::spawn(async move {
            tokio::time::sleep(CLOSE_DELAY).await;
            closing.cancel();
        });
    }

    /// Warum der Client gehen musste, wenn nicht von selbst.
    #[must_use]
    pub(crate) fn leave_reason(&self) -> (LeaveReason, Option<String>) {
        self.leave.lock().clone()
    }
}
