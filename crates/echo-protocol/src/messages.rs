//! Die Steuernachrichten. Jede geht in einem [`Envelope`] ueber den Stream;
//! Anfragen tragen eine `request_id` (> 0), und jede Antwort darauf —
//! `Reply`, `Pong`, `FileList` und so weiter — traegt dieselbe zurueck.
//! Ereignisse tragen 0.
//!
//! postcard schreibt die Variante als Index in Deklarationsreihenfolge:
//! neue Nachrichten gehoeren ans Ende, und jede Aenderung erhoeht
//! [`crate::Wire::VERSION`].

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChatScope {
    Server,
    Channel,
    Private,
}

/// Rollen statt Rechte-Matrix; die Reihenfolge ist die Rangfolge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Role {
    Guest,
    Member,
    Admin,
    Owner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaveReason {
    Left,
    Timeout,
    Kicked,
    Banned,
    ServerStopped,
}

/// Was ein Nutzer in seinen Kanal streamt (echo-62). Je Nutzer laeuft von
/// jeder Art hoechstens einer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamKind {
    Radio,
    Kino,
    /// Der geteilte Bildschirm (echo-18).
    Screen,
}

/// Wo ein Schluessel steht, wenn der Server Freigaben verlangt (echo-39).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalState {
    /// Hat angeklopft und wartet.
    Pending,
    Allowed,
    Denied,
}

/// Ein Kanal, wie beide Seiten ihn sehen. `order` ist die ID des Vorgaengers
/// unter demselben Elternkanal (0 = erster), damit die Sortierung des
/// Clients sich nicht von der des Servers unterscheidet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelDto {
    pub id: u64,
    pub parent: u64,
    pub order: u64,
    pub name: String,
    pub topic: Option<String>,
    pub has_password: bool,
    pub is_default: bool,
    pub permanent: bool,
    pub max_clients: i32,
    pub needed_talk_power: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientDto {
    pub id: u16,
    pub channel: u64,
    pub name: String,
    pub fingerprint: String,
    pub role: Role,
    pub input_muted: bool,
    pub output_muted: bool,
    pub away_message: Option<String>,
    pub talk_power: i32,
}

/// Ein laufender Stream. Er gehoert seinem Besitzer und laeuft in dessen
/// Kanal. `id` steht als Absender in seinen Ton- und Bildrahmen, damit jeder
/// Zuhoerer ihn einzeln regeln kann.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamDto {
    pub id: u16,
    pub owner: u16,
    pub kind: StreamKind,
    pub title: String,
}

/// Ein Eintrag im Dateibereich eines Kanals (echo-20). Ordner tragen keine Groesse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntryDto {
    pub id: String,
    pub name: String,
    pub is_folder: bool,
    pub size: u64,
    pub uploaded_by_name: String,
    /// Unix-Millisekunden.
    pub uploaded_at: i64,
}

/// Ein privates Gespraech auf dem Server: mit wem, unter welchem Namen
/// zuletzt, und bis wohin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmSummary {
    pub peer: String,
    pub peer_name: String,
    pub last_seq: i64,
}

/// Eine Tafel eines Kanals (echo-183).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardDto {
    pub id: String,
    pub name: String,
}

/// Eine Form: ihr JSON, oder `None` fuer "geloescht". Der Server liest es nicht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeDto {
    pub id: String,
    pub json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDto {
    pub fingerprint: String,
    pub name: String,
    /// Unix-Millisekunden.
    pub requested: i64,
    pub state: ApprovalState,
}

/// Ein Bild im Chat (echo-166). `id` vergibt der Server; Breite und Hoehe
/// nennt der Absender, damit der Verlauf den Platz reserviert, bevor das
/// Bild geladen ist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImageDto {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
}

/// Welches Archiv (echo-182): Serverchat, Kanal oder privates Gespraech.
/// `peer` ist bei privaten Gespraechen der Fingerabdruck des Gegenuebers aus
/// Sicht des Empfaengers, sonst leer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArchiveRef {
    pub scope: ChatScope,
    pub channel: u64,
    pub peer: String,
}

/// Eine gespeicherte Nachricht. `seq` ist lueckenlos je Archiv, `time`
/// Serverzeit in Unix-Millisekunden, `author` der Fingerabdruck. Mit
/// `image` ist `text` leer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessageDto {
    pub seq: i64,
    pub time: i64,
    pub author: String,
    pub author_name: String,
    pub text: String,
    pub image: Option<ChatImageDto>,
}

/// Eine Nachricht mit der Nummer der Anfrage, zu der sie gehoert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// > 0 bei Anfragen und ihren Antworten, 0 bei Ereignissen.
    pub request_id: u32,
    pub message: Message,
}

impl Envelope {
    /// Ein Ereignis ohne Anfrage.
    #[must_use]
    pub fn event(message: Message) -> Self {
        Self {
            request_id: 0,
            message,
        }
    }
}

/// Alle Steuernachrichten. Die Reihenfolge ist Teil des Formats.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    // --- Handschlag ---------------------------------------------------------
    /// Client -> Server, die erste Nachricht ueberhaupt: ein QUIC-Stream
    /// existiert fuer die Gegenseite erst, wenn Daten fliessen.
    Open {
        version: u32,
    },
    /// Server -> Client, Antwort auf `Open`. `app_version` steht in
    /// Fehlermeldungen, damit man weiss, welche Seite aelter ist.
    Challenge {
        nonce: String,
        server_name: String,
        version: u32,
        app_version: String,
        needs_password: bool,
    },
    /// Client -> Server: Identitaet beweisen. `signature` ist ECDSA ueber
    /// [`crate::hello_payload`], damit die Antwort weder auf einen anderen
    /// Server noch auf einen anderen Namen passt. Das Serverpasswort selbst
    /// verlaesst den Client nicht, nur sein Beweis (echo-37).
    Hello {
        nickname: String,
        public_key: String,
        signature: String,
        password_proof: Option<String>,
        default_channel: u64,
        default_channel_password: Option<String>,
        input_muted: bool,
        output_muted: bool,
    },
    /// Server -> Client: angenommen, mit dem kompletten Anfangszustand.
    Welcome {
        client_id: u16,
        server_name: String,
        welcome_message: String,
        role: Role,
        channels: Vec<ChannelDto>,
        clients: Vec<ClientDto>,
        streams: Vec<StreamDto>,
    },
    /// Antwort auf eine Anfrage.
    Reply {
        ok: bool,
        error: Option<String>,
    },
    Ping,
    Pong,

    // --- Anfragen des Clients -----------------------------------------------
    JoinChannel {
        channel_id: u64,
        password: Option<String>,
    },
    SendChat {
        scope: ChatScope,
        target: u16,
        text: String,
    },
    SetState {
        input_muted: Option<bool>,
        output_muted: Option<bool>,
        away: Option<bool>,
        away_message: Option<String>,
        nickname: Option<String>,
    },
    Poke {
        client_id: u16,
        text: String,
    },
    Kick {
        client_id: u16,
        from_server: bool,
        reason: String,
    },
    Ban {
        client_id: u16,
        reason: String,
        duration_seconds: u32,
    },
    CreateChannel {
        name: String,
        parent: u64,
        permanent: bool,
        password: Option<String>,
        max_clients: i32,
        topic: Option<String>,
    },
    /// Einen bestehenden Kanal aendern (echo-30). Das Passwort verraet der
    /// Server nie, der Client kann es also nicht vorbelegen: `change_password`
    /// false laesst es, true setzt `password` — leer heisst "kein Passwort".
    EditChannel {
        channel_id: u64,
        name: String,
        topic: Option<String>,
        change_password: bool,
        password: Option<String>,
        needed_talk_power: i32,
    },
    DeleteChannel {
        channel_id: u64,
    },
    MoveClient {
        client_id: u16,
        channel_id: u64,
    },
    SetRole {
        fingerprint: String,
        role: Role,
    },

    // --- Dateien (echo-20) --------------------------------------------------
    /// `folder` ist "" fuer die Wurzel, sonst ein Pfad mit `/`.
    ListFiles {
        channel: u64,
        folder: String,
    },
    /// `used` und `limit` sind die serverweiten Zahlen.
    FileList {
        channel: u64,
        folder: String,
        entries: Vec<FileEntryDto>,
        used: u64,
        limit: u64,
    },
    CreateFolder {
        channel: u64,
        folder: String,
        name: String,
    },
    RenameFile {
        file_id: String,
        name: String,
    },
    DeleteFile {
        file_id: String,
    },
    /// Einen Eintrag in einen Ordner legen, auch eines anderen Kanals
    /// (echo-173); ein Ordner nimmt seinen Inhalt mit.
    MoveFile {
        file_id: String,
        channel: u64,
        folder: String,
    },
    /// Im Dateibereich dieses Kanals hat sich etwas geaendert.
    FilesChanged {
        channel: u64,
    },
    /// Platz fuer eine Datei anfragen; die Antwort ist ein `TransferTicket`.
    RequestUpload {
        channel: u64,
        folder: String,
        name: String,
        size: u64,
    },
    /// Eine Datei ab `offset` holen. `stream` heisst, dass Kino, Radio oder
    /// Bildbetrachter sie direkt lesen und oft mittendrin auflegen (echo-178);
    /// das ist dann kein Abbruch.
    RequestDownload {
        file_id: String,
        offset: u64,
        stream: bool,
    },
    /// Ein Bild in den Chat schicken (echo-166); die Antwort ist ein
    /// `TransferTicket`, dessen `name` die Kennung des Bildes ist.
    RequestChatImage {
        scope: ChatScope,
        target: u16,
        size: u64,
        width: u32,
        height: u32,
    },
    RequestImageDownload {
        archive: ArchiveRef,
        image_id: String,
    },

    // --- Verlauf (echo-182) -------------------------------------------------
    /// Alles nach `after_seq`, zum Nachholen; `ChatHistory::more` sagt, dass
    /// danach noch etwas kommt.
    SyncChat {
        archive: ArchiveRef,
        after_seq: i64,
    },
    /// Die Seite direkt vor `before_seq`, beim Hochscrollen.
    LoadHistory {
        archive: ArchiveRef,
        before_seq: i64,
    },
    ChatHistory {
        archive: ArchiveRef,
        messages: Vec<ChatMessageDto>,
        more: bool,
    },
    /// Nach dem `Welcome`: die eigenen privaten Gespraeche zum Nachholen.
    DmConversations {
        conversations: Vec<DmSummary>,
    },

    // --- Tafeln (echo-183) --------------------------------------------------
    ListBoards {
        channel: u64,
    },
    BoardList {
        channel: u64,
        boards: Vec<BoardDto>,
    },
    CreateBoard {
        channel: u64,
        name: String,
    },
    RenameBoard {
        board_id: String,
        name: String,
    },
    DeleteBoard {
        board_id: String,
    },
    BoardsChanged {
        channel: u64,
    },
    /// Eine Tafel ansehen. Antwort ist `BoardSnapshot`; die Formen folgen als
    /// `BoardEdited` mit `by` 0 in Portionen, weil eine volle Tafel groesser
    /// sein kann als eine Nachricht.
    OpenBoard {
        board_id: String,
    },
    BoardSnapshot {
        board_id: String,
        name: String,
        participants: Vec<u16>,
    },
    CloseBoard {
        board_id: String,
    },
    /// Mitmachen oder wieder nur zusehen; erst wer mitmacht, darf zeichnen.
    JoinBoard {
        board_id: String,
        join: bool,
    },
    BoardPresence {
        board_id: String,
        participants: Vec<u16>,
    },
    /// Aenderungen an Formen; das Echo ist `BoardEdited`.
    EditBoard {
        board_id: String,
        changes: Vec<ShapeDto>,
    },
    /// Aenderungen in der Reihenfolge des Servers, auch an den, der sie machte.
    BoardEdited {
        board_id: String,
        by: u16,
        changes: Vec<ShapeDto>,
    },
    /// Wo der eigene Zeiger ist; `laser` zieht eine kurze Spur. Fluechtig.
    BoardPointer {
        board_id: String,
        x: f32,
        y: f32,
        laser: bool,
    },
    BoardPointerMoved {
        board_id: String,
        by: u16,
        x: f32,
        y: f32,
        laser: bool,
    },

    // --- Medien und Daten ---------------------------------------------------
    /// Eine Medienverbindung fuer Radio, Kino oder Bildschirm anmelden
    /// (echo-16, echo-62); die Antwort ist ein `StreamTicket`.
    RequestStream {
        kind: StreamKind,
        title: String,
    },
    /// `stream_id` steht spaeter als Absender in den Rahmen dieses Streams.
    StreamTicket {
        token: String,
        stream_id: u16,
    },
    /// Die erste Nachricht auf einer Medienverbindung, statt `Open`.
    StreamHello {
        token: String,
    },
    /// Die Eintrittskarte fuer die Datenverbindung: einmal gueltig, kurz haltbar.
    TransferTicket {
        token: String,
        name: String,
        size: u64,
    },
    /// Die erste Nachricht auf einer Datenverbindung, statt `Open`. Danach
    /// fliessen auf demselben Stream die reinen Bytes.
    TransferHello {
        token: String,
    },

    // --- Freigaben (echo-39) ------------------------------------------------
    ListApprovals,
    ApprovalList {
        entries: Vec<ApprovalDto>,
    },
    DecideApproval {
        fingerprint: String,
        allow: bool,
    },
    /// Die Liste hat sich geaendert, `pending` wartet noch.
    ApprovalsChanged {
        pending: u32,
    },

    // --- Ereignisse des Servers ---------------------------------------------
    ChannelAdded {
        channel: ChannelDto,
    },
    ChannelRemoved {
        channel_id: u64,
    },
    ChannelChanged {
        channel: ChannelDto,
    },
    ClientJoined {
        client: ClientDto,
    },
    ClientLeft {
        client_id: u16,
        reason: LeaveReason,
        detail: Option<String>,
    },
    ClientMoved {
        client_id: u16,
        channel_id: u64,
        invoker_id: u16,
    },
    ClientChanged {
        client: ClientDto,
    },
    StreamStarted {
        stream: StreamDto,
    },
    StreamEnded {
        stream_id: u16,
    },
    /// Bitte um ein Keyframe (echo-83): wer einsteigt oder den Faden verlor,
    /// braucht ein ganzes Bild. Vom Zuschauer an den Server, von dort an den
    /// Besitzer des Streams.
    KeyframeRequest {
        stream_id: u16,
    },
    /// Einem Kino- oder Bildschirm-Stream zuschauen (echo-87); Bilder und
    /// Filmton gehen nur an Zuschauer.
    WatchStream {
        stream_id: u16,
    },
    UnwatchStream {
        stream_id: u16,
    },
    /// Eine neue Nachricht, schon im Archiv. Auch der Absender bekommt sie,
    /// erst damit hat er ihre Nummer.
    ChatEvent {
        archive: ArchiveRef,
        from_id: u16,
        message: ChatMessageDto,
    },
    Poked {
        from_id: u16,
        from_name: String,
        text: String,
    },
    /// Geworfen oder gebannt; die Verbindung schliesst danach.
    Kicked {
        from_server: bool,
        reason: String,
        invoker_id: u16,
        invoker_name: Option<String>,
    },
}
