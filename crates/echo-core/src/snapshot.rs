//! Der Stand des Servers, wie die Oberflaeche ihn liest: unveraenderlich,
//! je Aenderung neu erzeugt und atomar ersetzt. Die UI liest genau einen
//! Schnappschuss je Frame.

use echo_protocol::{ChannelDto, ClientDto, Role, StreamDto, StreamKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInfo {
    pub id: u64,
    pub parent: u64,
    /// Kanal-ID des Vorgaengers unter demselben Elternkanal, 0 = erster.
    pub order: u64,
    pub name: String,
    pub topic: Option<String>,
    pub has_password: bool,
    pub is_default: bool,
    /// -1 = unbegrenzt.
    pub max_clients: i32,
    pub needed_talk_power: i32,
}

impl ChannelInfo {
    /// Spacer wie im Originalclient: "[cspacer]Text", "[spacer0]---".
    #[must_use]
    pub fn is_spacer(&self) -> bool {
        self.name.starts_with('[')
            && self.name.contains(']')
            && self.name.to_lowercase().contains("spacer")
    }

    #[must_use]
    pub fn is_centered_spacer(&self) -> bool {
        self.is_spacer() && self.name.to_lowercase().starts_with("[cspacer")
    }

    /// Der sichtbare Name ohne Spacer-Praefix.
    #[must_use]
    pub fn display_name(&self) -> &str {
        if self.is_spacer() {
            self.name
                .split_once(']')
                .map_or(&self.name, |(_, rest)| rest)
        } else {
            &self.name
        }
    }
}

impl From<&ChannelDto> for ChannelInfo {
    fn from(c: &ChannelDto) -> Self {
        Self {
            id: c.id,
            parent: c.parent,
            order: c.order,
            name: c.name.clone(),
            topic: c.topic.clone(),
            has_password: c.has_password,
            is_default: c.is_default,
            max_clients: c.max_clients,
            needed_talk_power: c.needed_talk_power,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInfo {
    pub id: u16,
    pub channel: u64,
    pub name: String,
    /// Der Fingerabdruck der Identitaet.
    pub fingerprint: String,
    pub input_muted: bool,
    pub output_muted: bool,
    pub away_message: Option<String>,
    pub talk_power: i32,
    /// Die Rolle am Server (echo-5).
    pub role: Role,
    pub is_self: bool,
}

impl ClientInfo {
    fn from_dto(c: &ClientDto, own: u16) -> Self {
        Self {
            id: c.id,
            channel: c.channel,
            name: c.name.clone(),
            fingerprint: c.fingerprint.clone(),
            input_muted: c.input_muted,
            output_muted: c.output_muted,
            away_message: c.away_message.clone(),
            talk_power: c.talk_power,
            role: c.role,
            is_self: c.id == own,
        }
    }

    #[must_use]
    pub fn is_away(&self) -> bool {
        self.away_message.is_some()
    }
}

/// Ein laufender Stream (echo-62). Er gehoert `owner` und laeuft in dessen
/// Kanal; `id` ist der Absender seiner Ton- und Bildrahmen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    pub id: u16,
    pub owner: u16,
    pub kind: StreamKind,
    pub title: String,
}

impl From<&StreamDto> for StreamInfo {
    fn from(s: &StreamDto) -> Self {
        Self {
            id: s.id,
            owner: s.owner,
            kind: s.kind,
            title: s.title.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerSnapshot {
    pub server_name: String,
    pub welcome_message: String,
    pub own_client_id: u16,
    pub own_role: Option<Role>,
    pub channels: Vec<ChannelInfo>,
    pub clients: Vec<ClientInfo>,
    pub streams: Vec<StreamInfo>,
    /// Laufende Nummer; steigt je Aenderung — die UI erkennt daran neue Staende.
    pub version: u64,
}

impl ServerSnapshot {
    pub(crate) fn build(
        server_name: &str,
        welcome_message: &str,
        own: u16,
        channels: &[ChannelDto],
        clients: &[ClientDto],
        streams: &[StreamDto],
        version: u64,
    ) -> Self {
        let mut channels: Vec<_> = channels.iter().map(ChannelInfo::from).collect();
        channels.sort_by_key(|c| c.id);
        let mut clients: Vec<_> = clients
            .iter()
            .map(|c| ClientInfo::from_dto(c, own))
            .collect();
        clients.sort_by_key(|c| c.id);
        let own_role = clients.iter().find(|c| c.id == own).map(|c| c.role);
        Self {
            server_name: server_name.to_owned(),
            welcome_message: welcome_message.to_owned(),
            own_client_id: own,
            own_role,
            channels,
            clients,
            streams: streams.iter().map(StreamInfo::from).collect(),
            version,
        }
    }

    #[must_use]
    pub fn me(&self) -> Option<&ClientInfo> {
        self.client(self.own_client_id)
    }

    #[must_use]
    pub fn channel(&self, id: u64) -> Option<&ChannelInfo> {
        self.channels.iter().find(|c| c.id == id)
    }

    #[must_use]
    pub fn client(&self, id: u16) -> Option<&ClientInfo> {
        self.clients.iter().find(|c| c.id == id)
    }

    #[must_use]
    pub fn own_channel(&self) -> Option<&ChannelInfo> {
        self.me().and_then(|me| self.channel(me.channel))
    }

    /// Der Stream dieser Art, den ein Nutzer gerade laufen hat.
    #[must_use]
    pub fn stream_of(&self, owner: u16, kind: StreamKind) -> Option<&StreamInfo> {
        self.streams
            .iter()
            .find(|s| s.owner == owner && s.kind == kind)
    }

    /// Der Name eines Kanals fuer Meldungen.
    #[must_use]
    pub fn channel_name(&self, id: u64) -> String {
        self.channel(id)
            .map_or_else(|| format!("#{id}"), |c| c.display_name().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(name: &str) -> ChannelInfo {
        ChannelInfo {
            id: 1,
            parent: 0,
            order: 0,
            name: name.into(),
            topic: None,
            has_password: false,
            is_default: false,
            max_clients: -1,
            needed_talk_power: 0,
        }
    }

    #[test]
    fn spacers_show_only_their_text() {
        assert!(channel("[cspacer]Mitte").is_centered_spacer());
        assert_eq!(channel("[cspacer]Mitte").display_name(), "Mitte");
        assert!(channel("[spacer0]---").is_spacer());
        assert!(!channel("[spacer0]---").is_centered_spacer());
        assert!(!channel("[Raum]").is_spacer());
        assert_eq!(channel("Lobby").display_name(), "Lobby");
    }

    #[test]
    fn finds_its_way_around() {
        let dto = ClientDto {
            id: 7,
            channel: 1,
            name: "Alice".into(),
            fingerprint: "SHA256:a".into(),
            role: Role::Owner,
            input_muted: false,
            output_muted: false,
            away_message: Some("weg".into()),
            talk_power: 0,
        };
        let lobby = ChannelDto {
            id: 1,
            parent: 0,
            order: 0,
            name: "Lobby".into(),
            topic: None,
            has_password: false,
            is_default: true,
            permanent: true,
            max_clients: -1,
            needed_talk_power: 0,
        };
        let radio = StreamDto {
            id: 3,
            owner: 7,
            kind: StreamKind::Radio,
            title: "Musik".into(),
        };
        let snapshot = ServerSnapshot::build("S", "W", 7, &[lobby], &[dto], &[radio], 1);
        assert!(snapshot.me().unwrap().is_self);
        assert!(snapshot.me().unwrap().is_away());
        assert_eq!(snapshot.own_role, Some(Role::Owner));
        assert_eq!(snapshot.own_channel().unwrap().name, "Lobby");
        assert_eq!(snapshot.channel_name(9), "#9");
        assert_eq!(
            snapshot.stream_of(7, StreamKind::Radio).unwrap().title,
            "Musik"
        );
        assert!(snapshot.stream_of(7, StreamKind::Screen).is_none());
    }
}
