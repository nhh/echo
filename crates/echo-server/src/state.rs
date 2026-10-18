//! Der Zustand eines Servers: er fuehrt Kanalbaum und Clientliste zusammen,
//! prueft Rechte und verschickt die Ereignisse. Alles hinter einer Sperre,
//! jede oeffentliche Methode ist eine atomare Aenderung; die Verbindungen
//! puffern ihre Sendungen, hier blockiert nichts.
//!
//! Sprachpakete gehen nicht durch diese Sperre: [`ServerState::members`]
//! liest den unveraenderlichen Schnappschuss der Clientliste.

use std::collections::HashSet;
use std::sync::Arc;

use arc_swap::{ArcSwap, Guard};
use echo_protocol::{
    ApprovalDto, ApprovalState, ArchiveRef, ChatImageDto, ChatMessageDto, ChatScope, DmSummary,
    Envelope, LeaveReason, Message, PasswordProof, Rights, Role, encode,
};
use echo_storage::{ArchivedMessage, ChannelArchive, StorageError};
use parking_lot::Mutex;

use crate::archives::ArchiveSet;
use crate::channels::{ChannelRegistry, NewChannel};
use crate::client::{Client, ClientLink, Frame, NewClient};
use crate::clients::{ClientRegistry, Members};
use crate::stores::{Approval, BanEntry, Member, ServerDatabase, ServerSettings};
use crate::ticks;

const MAX_CHAT_LENGTH: usize = 4000;

/// So viele Nachrichten hat eine Seite des Verlaufs (echo-182).
const HISTORY_PAGE: u32 = 200;

/// Was ein Client beim Verbinden verlangt, schon mit gepruefter Signatur.
#[derive(Debug, Clone)]
pub(crate) struct Admission {
    pub(crate) fingerprint: String,
    pub(crate) nickname: String,
    pub(crate) password_proof: Option<String>,
    pub(crate) default_channel: u64,
    pub(crate) default_channel_password: Option<String>,
    pub(crate) input_muted: bool,
    pub(crate) output_muted: bool,
    /// Die Nonce der Challenge; sie bindet den Passwortbeweis an diese Verbindung.
    pub(crate) nonce: String,
}

/// Was ein Client an seinem Zustand aendert; `None` laesst ein Feld, wie es ist.
#[derive(Debug, Clone, Default)]
pub(crate) struct StateChange {
    pub(crate) input_muted: Option<bool>,
    pub(crate) output_muted: Option<bool>,
    pub(crate) away: Option<bool>,
    pub(crate) away_message: Option<String>,
    pub(crate) nickname: Option<String>,
}

/// Ein neuer Kanal aus `CreateChannel`.
#[derive(Debug, Clone)]
pub(crate) struct ChannelRequest {
    pub(crate) name: String,
    pub(crate) parent: u64,
    pub(crate) permanent: bool,
    pub(crate) password: Option<String>,
    pub(crate) max_clients: i32,
    pub(crate) topic: Option<String>,
}

/// Eine Aenderung aus `EditChannel`.
#[derive(Debug, Clone)]
pub(crate) struct ChannelEdit {
    pub(crate) channel_id: u64,
    pub(crate) name: String,
    pub(crate) topic: Option<String>,
    pub(crate) change_password: bool,
    pub(crate) password: Option<String>,
    pub(crate) needed_talk_power: i32,
}

/// Ein Grund fuer den Nutzer, warum etwas nicht ging.
pub(crate) type Refusal = String;

type Log = Box<dyn Fn(&str) + Send + Sync>;

struct Inner {
    settings: ServerSettings,
    db: ServerDatabase,
    archives: Arc<ArchiveSet>,
    channels: ChannelRegistry,
    clients: ClientRegistry,
}

pub(crate) struct ServerState {
    inner: Mutex<Inner>,
    members: Arc<ArcSwap<Members>>,
    log: Log,
}

impl std::fmt::Debug for ServerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerState").finish_non_exhaustive()
    }
}

impl ServerState {
    pub(crate) fn new(
        settings: ServerSettings,
        db: ServerDatabase,
        archives: Arc<ArchiveSet>,
        log: Log,
    ) -> Result<Self, StorageError> {
        let (channels, changed) = ChannelRegistry::load(db.channels()?);
        if changed {
            db.save_channels(&channels.stored())?;
        }
        let permanent: HashSet<u64> = channels
            .all()
            .filter(|c| c.permanent)
            .map(|c| c.id)
            .collect();
        archives.retire_orphans(&permanent);

        let members = Arc::new(ArcSwap::from_pointee(Members::new()));
        Ok(Self {
            inner: Mutex::new(Inner {
                settings,
                db,
                archives,
                channels,
                clients: ClientRegistry::new(Arc::clone(&members)),
            }),
            members,
            log,
        })
    }

    /// Wer in welchem Kanal ist — ohne Sperre, fuer die Sprachverteilung.
    #[must_use]
    pub(crate) fn members(&self) -> Guard<Arc<Members>> {
        self.members.load()
    }

    #[must_use]
    pub(crate) fn server_name(&self) -> String {
        self.inner.lock().settings.name.clone()
    }

    /// Steht schon in der Challenge, damit der Client nach dem Passwort
    /// fragen kann, statt blind abgewiesen zu werden.
    #[must_use]
    pub(crate) fn needs_password(&self) -> bool {
        self.inner
            .lock()
            .settings
            .password
            .as_ref()
            .is_some_and(|p| !p.is_empty())
    }

    #[must_use]
    pub(crate) fn client_count(&self) -> usize {
        self.inner.lock().clients.count()
    }

    pub(crate) fn log(&self, line: &str) {
        (self.log)(line);
    }

    // --- Verbinden und Trennen ------------------------------------------------

    /// Prueft Bann, Passwort, Freigabe und Platz, vergibt ID, Name und Rolle
    /// und setzt den Client in seinen Startkanal. Liefert den Client und sein
    /// `Welcome`.
    pub(crate) fn admit(
        &self,
        admission: &Admission,
        link: ClientLink,
    ) -> Result<(Arc<Client>, Message), Refusal> {
        let mut inner = self.inner.lock();
        if let Some(ban) = inner.db.active_ban(&admission.fingerprint) {
            return Err(format!(
                "Gebannt bis {}: {}",
                ticks::display(ban.until),
                ban.reason
            ));
        }

        if let Some(password) = inner.settings.password.as_deref().filter(|p| !p.is_empty()) {
            let expected = PasswordProof::compute(&admission.nonce, password);
            if !PasswordProof::matches(admission.password_proof.as_deref(), &expected) {
                return Err("Falsches Serverpasswort.".into());
            }
        }

        if let Some(waiting) = self.knock(&mut inner, admission) {
            return Err(waiting);
        }

        if inner.clients.count() >= inner.settings.max_clients as usize {
            return Err("Der Server ist voll.".into());
        }

        let role = resolve_role(&inner, &admission.fingerprint);
        let channel = choose_start_channel(&inner, admission);
        let id = inner.clients.next_id().ok_or("Der Server ist voll.")?;
        let name = inner.clients.unique_name(&admission.nickname, None);
        let client = Arc::new(Client::new(
            NewClient {
                id,
                fingerprint: admission.fingerprint.clone(),
                name: name.clone(),
                role,
                channel,
                input_muted: admission.input_muted,
                output_muted: admission.output_muted,
            },
            link,
        ));
        inner.clients.add(Arc::clone(&client));
        self.save_member(&mut inner, &client);

        broadcast(
            &inner,
            &Message::ClientJoined {
                client: client.to_dto(),
            },
            Some(id),
        );
        let channel_name = inner
            .channels
            .get(channel)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        inner.db.log_event(
            "join",
            &format!("{name} ({}) -> {channel_name}", admission.fingerprint),
        );

        let welcome = Message::Welcome {
            client_id: id,
            server_name: inner.settings.name.clone(),
            welcome_message: inner.settings.welcome_message.clone(),
            role,
            channels: inner
                .channels
                .sorted()
                .into_iter()
                .map(crate::channels::Channel::to_dto)
                .collect(),
            clients: inner.clients.all().iter().map(|c| c.to_dto()).collect(),
            streams: Vec::new(),
        };
        Ok((client, welcome))
    }

    /// Nach dem `Welcome`: wie viele anklopfen (fuer die Moderation) und
    /// welche privaten Gespraeche es gibt, damit der Client nachholen kann.
    pub(crate) fn greet(&self, client: &Client) {
        let inner = self.inner.lock();
        let pending = inner.db.pending_approvals();
        if Rights::may_approve(client.role()) && pending > 0 {
            client.send(Message::ApprovalsChanged { pending });
        }

        let conversations = inner
            .archives
            .peers_of(client.fingerprint())
            .into_iter()
            .map(|peer| {
                let last_seq = inner
                    .archives
                    .dm(client.fingerprint(), &peer, false)
                    .ok()
                    .flatten()
                    .and_then(|dm| dm.last_seq().ok())
                    .unwrap_or(0);
                DmSummary {
                    peer_name: inner
                        .db
                        .member(&peer)
                        .map_or_else(|| "Unbekannt".into(), |m| m.last_name.clone()),
                    peer,
                    last_seq,
                }
            })
            .collect();
        client.send(Message::DmConversations { conversations });
    }

    pub(crate) fn remove(&self, client: &Client, reason: LeaveReason, detail: Option<String>) {
        let mut inner = self.inner.lock();
        if !inner.clients.remove(client.id()) {
            return;
        }
        broadcast(
            &inner,
            &Message::ClientLeft {
                client_id: client.id(),
                reason,
                detail: detail.clone(),
            },
            None,
        );
        inner.db.log_event(
            "leave",
            &format!(
                "{}: {reason:?} {}",
                client.name(),
                detail.unwrap_or_default()
            ),
        );
        cleanup_temporary(&mut inner, client.channel());
    }

    // --- Freigaben (echo-39) --------------------------------------------------

    /// Darf dieser Schluessel herein? Bekannte Mitglieder kommen immer durch,
    /// damit das Einschalten der Freigabe niemanden aussperrt; ein unbekannter
    /// klopft an und wartet.
    fn knock(&self, inner: &mut Inner, admission: &Admission) -> Option<Refusal> {
        if !inner.settings.require_approval {
            return None;
        }

        // Eine Ablehnung wiegt schwerer als alles andere: sie nimmt auch
        // jemandem den Zutritt, der frueher schon einmal hier war.
        let known = inner.db.approval(&admission.fingerprint).map(|a| a.state);
        match known {
            Some(ApprovalState::Denied) => {
                return Some("Der Server hat deine Anfrage abgelehnt.".into());
            }
            Some(ApprovalState::Allowed) => return None,
            _ => {}
        }

        // Auf einem frischen Server kommt der erste Schluessel durch, sonst
        // sperrte die Freigabe den Besitzer aus, bevor es ihn gibt.
        if !inner.db.has_owner() || inner.db.member(&admission.fingerprint).is_some() {
            return None;
        }

        if known.is_none() {
            let saved = inner.db.save_approval(Approval {
                fingerprint: admission.fingerprint.clone(),
                name: admission.nickname.clone(),
                state: ApprovalState::Pending,
                requested: ticks::now(),
                decided: 0,
                decided_by: String::new(),
            });
            let line = format!(
                "{} ({}) klopft an",
                admission.nickname, admission.fingerprint
            );
            inner.db.log_event("knock", &line);
            self.log(&line);
            if saved.is_ok() {
                notify_approvals(inner);
            }
        }
        Some("Du klopfst an. Ein Admin dieses Servers muss dich erst freischalten.".into())
    }

    pub(crate) fn list_approvals(&self, by: &Client) -> Result<Message, Refusal> {
        let inner = self.inner.lock();
        if !Rights::may_approve(by.role()) {
            return Err("Keine Berechtigung.".into());
        }
        let mut entries: Vec<_> = inner.db.approvals().cloned().collect();
        entries.sort_by_key(|a| {
            (
                a.state != ApprovalState::Pending,
                std::cmp::Reverse(a.requested),
            )
        });
        Ok(Message::ApprovalList {
            entries: entries
                .into_iter()
                .map(|a| ApprovalDto {
                    fingerprint: a.fingerprint,
                    name: a.name,
                    requested: ticks::to_unix_millis(a.requested),
                    state: a.state,
                })
                .collect(),
        })
    }

    pub(crate) fn decide_approval(
        &self,
        by: &Client,
        fingerprint: &str,
        allow: bool,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        if !Rights::may_approve(by.role()) {
            return Err("Keine Berechtigung.".into());
        }
        let approval = inner
            .db
            .approval(fingerprint)
            .cloned()
            .ok_or("Diesen Schlüssel kennt der Server nicht.")?;
        let name = approval.name.clone();
        inner
            .db
            .save_approval(Approval {
                state: if allow {
                    ApprovalState::Allowed
                } else {
                    ApprovalState::Denied
                },
                decided: ticks::now(),
                decided_by: by.name(),
                ..approval
            })
            .map_err(storage_refusal)?;
        let verdict = if allow { "lässt herein" } else { "weist ab" };
        inner.db.log_event(
            "knock",
            &format!("{} {verdict}: {name} ({fingerprint})", by.name()),
        );
        notify_approvals(&inner);
        Ok(())
    }

    // --- Kanaele --------------------------------------------------------------

    pub(crate) fn join(
        &self,
        client: &Client,
        channel_id: u64,
        password: Option<&str>,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        join_locked(&mut inner, client, channel_id, password, 0)
    }

    pub(crate) fn create_channel(
        &self,
        by: &Client,
        request: ChannelRequest,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        if !Rights::may_create_channel(by.role(), request.permanent) {
            return Err(if request.permanent {
                "Dauerhafte Kanäle darf nur ein Admin anlegen.".into()
            } else {
                "Gäste dürfen keine Kanäle anlegen.".into()
            });
        }
        if request.parent != 0 && !inner.channels.exists(request.parent) {
            return Err("Elternkanal existiert nicht.".into());
        }
        let name = request.name.trim().to_owned();
        if !ChannelRegistry::is_valid_name(&name) {
            return Err("Ungültiger Kanalname.".into());
        }

        let channel = inner
            .channels
            .add(NewChannel {
                name,
                parent: request.parent,
                permanent: request.permanent,
                password: request.password,
                max_clients: request.max_clients,
                topic: request.topic,
                created_by: by.id(),
            })
            .clone();
        if channel.permanent {
            persist_channels(&inner)?;
        }
        broadcast(
            &inner,
            &Message::ChannelAdded {
                channel: channel.to_dto(),
            },
            None,
        );
        let kind = if channel.permanent {
            "dauerhaft"
        } else {
            "temporaer"
        };
        inner.db.log_event(
            "channel",
            &format!("{} legt '{}' an ({kind})", by.name(), channel.name),
        );
        Ok(())
    }

    pub(crate) fn edit_channel(&self, by: &Client, edit: ChannelEdit) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        let channel = inner
            .channels
            .get(edit.channel_id)
            .ok_or("Kanal existiert nicht.")?;
        let own = !channel.permanent && channel.created_by == by.id();
        if !Rights::may_edit_channel(by.role(), own) {
            return Err("Keine Berechtigung.".into());
        }
        let name = edit.name.trim().to_owned();
        if !ChannelRegistry::is_valid_name(&name) {
            return Err("Ungültiger Kanalname.".into());
        }

        let Some(channel) = inner.channels.get_mut(edit.channel_id) else {
            return Err("Kanal existiert nicht.".into());
        };
        channel.name = name;
        channel.topic = edit
            .topic
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty());
        channel.needed_talk_power = edit.needed_talk_power.max(0);
        if edit.change_password {
            channel.password = edit.password.filter(|p| !p.is_empty());
        }
        let dto = channel.to_dto();
        persist_channels(&inner)?;
        broadcast(
            &inner,
            &Message::ChannelChanged {
                channel: dto.clone(),
            },
            None,
        );
        inner
            .db
            .log_event("channel", &format!("{} ändert '{}'", by.name(), dto.name));
        Ok(())
    }

    pub(crate) fn delete_channel(&self, by: &Client, channel_id: u64) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        let channel = inner
            .channels
            .get(channel_id)
            .ok_or("Kanal existiert nicht.")?;
        if channel.is_default {
            return Err("Der Standardkanal bleibt.".into());
        }
        let own = !channel.permanent && channel.created_by == by.id();
        if !Rights::may_delete_channel(by.role(), own) {
            return Err("Keine Berechtigung.".into());
        }
        delete_recursive(&mut inner, channel_id);
        persist_channels(&inner)
    }

    // --- Chat und Zustand -----------------------------------------------------

    pub(crate) fn chat(&self, from: &Client, scope: ChatScope, target: u16, text: &str) {
        let clean = text.trim();
        if clean.is_empty() || clean.chars().count() > MAX_CHAT_LENGTH {
            return;
        }
        let inner = self.inner.lock();
        let Some(route) = route(&inner, from, scope, from.channel(), target) else {
            from.send(Message::ClientLeft {
                client_id: target,
                reason: LeaveReason::Left,
                detail: Some("nicht mehr online".into()),
            });
            return;
        };
        if let Err(error) = publish(&route, from, clean) {
            self.log(&format!("Chat nicht gespeichert: {error}"));
        }
    }

    /// Eine Seite eines Archivs: nach `after` (Nachholen) oder, wenn `before`
    /// groesser 0 ist, direkt davor (Hochscrollen).
    pub(crate) fn history(
        &self,
        by: &Client,
        archive: ArchiveRef,
        after: i64,
        before: i64,
    ) -> Result<Message, Refusal> {
        let inner = self.inner.lock();
        let Some(store) = readable(&inner, by, &archive)? else {
            return Ok(Message::ChatHistory {
                archive,
                messages: Vec::new(),
                more: false,
            });
        };

        let page = if before > 0 {
            store.before(before, HISTORY_PAGE + 1)
        } else {
            store.after(after, HISTORY_PAGE + 1)
        }
        .map_err(storage_refusal)?;
        let more = page.len() > HISTORY_PAGE as usize;
        let shown: Vec<_> = match (more, before > 0) {
            (false, _) => page,
            // Beim Hochscrollen fehlt die aelteste, beim Nachholen die neueste.
            (true, true) => page.into_iter().skip(1).collect(),
            (true, false) => page.into_iter().take(HISTORY_PAGE as usize).collect(),
        };
        let messages = shown.iter().map(|m| to_dto(&store, m)).collect();
        Ok(Message::ChatHistory {
            archive,
            messages,
            more,
        })
    }

    pub(crate) fn set_state(&self, client: &Client, change: StateChange) {
        let mut inner = self.inner.lock();
        if let Some(muted) = change.input_muted {
            client.set_input_muted(muted);
        }
        if let Some(muted) = change.output_muted {
            client.set_output_muted(muted);
        }
        if let Some(away) = change.away {
            client.set_away(away.then(|| change.away_message.clone().unwrap_or_default()));
        }
        if let Some(name) = change
            .nickname
            .filter(|n| !n.trim().is_empty() && *n != client.name())
        {
            let unique = inner.clients.unique_name(&name, Some(client.id()));
            client.set_name(unique);
            self.save_member(&mut inner, client);
        }
        broadcast(
            &inner,
            &Message::ClientChanged {
                client: client.to_dto(),
            },
            None,
        );
    }

    pub(crate) fn poke(&self, from: &Client, target: u16, text: &str) -> Result<(), Refusal> {
        let inner = self.inner.lock();
        let to = inner
            .clients
            .get(target)
            .ok_or("Client ist nicht online.")?;
        to.send(Message::Poked {
            from_id: from.id(),
            from_name: from.name(),
            text: text.trim().to_owned(),
        });
        Ok(())
    }

    // --- Moderation -----------------------------------------------------------

    pub(crate) fn kick(
        &self,
        by: &Client,
        target: u16,
        from_server: bool,
        reason: &str,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        let victim = find_victim(&inner, by, target, true)?;
        victim.send(Message::Kicked {
            from_server,
            reason: reason.to_owned(),
            invoker_id: by.id(),
            invoker_name: Some(by.name()),
        });
        if !from_server {
            let lobby = inner.channels.default_channel().id;
            return join_locked(&mut inner, &victim, lobby, None, by.id());
        }
        inner.db.log_event(
            "kick",
            &format!("{} wirft {} vom Server: {reason}", by.name(), victim.name()),
        );
        victim.close_later(LeaveReason::Kicked, format!("{}: {reason}", by.name()));
        Ok(())
    }

    pub(crate) fn ban(
        &self,
        by: &Client,
        target: u16,
        reason: &str,
        duration_seconds: u32,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        let victim = find_victim(&inner, by, target, false)?;
        let until = if duration_seconds == 0 {
            ticks::FOREVER
        } else {
            ticks::after_seconds(duration_seconds)
        };
        inner
            .db
            .add_ban(BanEntry {
                fingerprint: victim.fingerprint().to_owned(),
                name: victim.name(),
                reason: reason.to_owned(),
                until,
                by: by.fingerprint().to_owned(),
            })
            .map_err(storage_refusal)?;
        victim.send(Message::Kicked {
            from_server: true,
            reason: reason.to_owned(),
            invoker_id: by.id(),
            invoker_name: Some(by.name()),
        });
        inner.db.log_event(
            "ban",
            &format!(
                "{} bannt {} bis {}: {reason}",
                by.name(),
                victim.name(),
                ticks::display(until)
            ),
        );
        victim.close_later(LeaveReason::Banned, format!("{}: {reason}", by.name()));
        Ok(())
    }

    pub(crate) fn move_client(
        &self,
        by: &Client,
        target: u16,
        channel_id: u64,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        if target != by.id() && !Rights::may_move_others(by.role()) {
            return Err("Keine Berechtigung.".into());
        }
        let client = inner
            .clients
            .get(target)
            .ok_or("Client ist nicht online.")?;
        join_locked(&mut inner, &client, channel_id, None, by.id())
    }

    pub(crate) fn set_role(
        &self,
        by: &Client,
        fingerprint: &str,
        role: Role,
    ) -> Result<(), Refusal> {
        let mut inner = self.inner.lock();
        if !Rights::may_assign_role(by.role(), role) {
            return Err("Keine Berechtigung.".into());
        }
        let existing = inner
            .db
            .member(fingerprint)
            .cloned()
            .ok_or("Unbekannter Schlüssel.")?;
        if !Rights::outranks(by.role(), existing.role) && existing.fingerprint != by.fingerprint() {
            return Err("Gleiche oder höhere Rolle.".into());
        }
        let name = existing.last_name.clone();
        inner
            .db
            .save_member(Member { role, ..existing })
            .map_err(storage_refusal)?;
        for online in inner.clients.with_fingerprint(fingerprint) {
            online.set_role(role);
            broadcast(
                &inner,
                &Message::ClientChanged {
                    client: online.to_dto(),
                },
                None,
            );
        }
        inner
            .db
            .log_event("role", &format!("{} setzt {name} auf {role:?}", by.name()));
        Ok(())
    }

    fn save_member(&self, inner: &mut Inner, client: &Client) {
        let saved = inner.db.save_member(Member {
            fingerprint: client.fingerprint().to_owned(),
            role: client.role(),
            last_name: client.name(),
            last_seen: ticks::now(),
        });
        if let Err(error) = saved {
            self.log(&format!("Mitglied nicht gespeichert: {error}"));
        }
    }
}

// --- Hilfen unter der Sperre --------------------------------------------------

fn resolve_role(inner: &Inner, fingerprint: &str) -> Role {
    if let Some(member) = inner.db.member(fingerprint) {
        return member.role;
    }
    // Der erste Schluessel, den ein frischer Server sieht, ist der Besitzer.
    if inner.db.has_owner() {
        inner.settings.default_role
    } else {
        Role::Owner
    }
}

/// Der gewuenschte Kanal, wenn es ihn gibt, das Passwort stimmt und Platz
/// ist; sonst der Standardkanal. Das verhindert das Verbinden nicht — man
/// landet einfach in der Lobby.
fn choose_start_channel(inner: &Inner, admission: &Admission) -> u64 {
    let lobby = inner.channels.default_channel().id;
    let Some(wanted) = inner.channels.get(admission.default_channel) else {
        return lobby;
    };
    let password_ok = wanted.password.is_none()
        || wanted.password.as_deref() == admission.default_channel_password.as_deref();
    if password_ok && !is_full(inner, wanted.id) {
        wanted.id
    } else {
        lobby
    }
}

fn is_full(inner: &Inner, channel: u64) -> bool {
    inner.channels.get(channel).is_some_and(|c| {
        usize::try_from(c.max_clients)
            .is_ok_and(|limit| inner.clients.in_channel(channel).len() >= limit)
    })
}

fn join_locked(
    inner: &mut Inner,
    client: &Client,
    channel_id: u64,
    password: Option<&str>,
    invoker: u16,
) -> Result<(), Refusal> {
    let channel = inner
        .channels
        .get(channel_id)
        .ok_or("Kanal existiert nicht.")?;
    if channel.id == client.channel() {
        return Ok(());
    }

    // Verschiebt ein Admin jemanden, gelten Passwort und Limit nicht.
    let by_admin = invoker != 0 && invoker != client.id();
    let password_ok = channel.password.is_none() || channel.password.as_deref() == password;
    if !password_ok && !by_admin && !Rights::may_bypass_channel_password(client.role()) {
        return Err("Falsches Kanalpasswort.".into());
    }
    if is_full(inner, channel_id) && !by_admin {
        return Err("Der Kanal ist voll.".into());
    }

    let previous = client.channel();
    inner.clients.move_to(client, channel_id);
    broadcast(
        inner,
        &Message::ClientMoved {
            client_id: client.id(),
            channel_id,
            invoker_id: invoker,
        },
        None,
    );
    cleanup_temporary(inner, previous);
    Ok(())
}

/// Loescht den Kanal samt Unterkanaelen, holt die Anwesenden in den
/// Standardkanal und schliesst die Luecke in der Reihenfolge.
fn delete_recursive(inner: &mut Inner, channel_id: u64) {
    for child in inner.channels.children(channel_id) {
        delete_recursive(inner, child);
    }

    let lobby = inner.channels.default_channel().id;
    for member in inner.clients.in_channel(channel_id) {
        inner.clients.move_to(&member, lobby);
        broadcast(
            inner,
            &Message::ClientMoved {
                client_id: member.id(),
                channel_id: lobby,
                invoker_id: 0,
            },
            None,
        );
    }

    let Some(removed) = inner.channels.get(channel_id).cloned() else {
        return;
    };
    for next in inner.channels.successors(channel_id) {
        if let Some(channel) = inner.channels.get_mut(next) {
            channel.order = removed.order;
            let dto = channel.to_dto();
            broadcast(inner, &Message::ChannelChanged { channel: dto }, None);
        }
    }
    inner.channels.remove(channel_id);
    inner.archives.retire_channel(channel_id, removed.permanent);
    broadcast(inner, &Message::ChannelRemoved { channel_id }, None);
}

/// Ein temporaerer Kanal verschwindet, sobald ihn der Letzte verlaesst und
/// kein Unterkanal mehr daran haengt.
fn cleanup_temporary(inner: &mut Inner, channel_id: u64) {
    let empty_temporary = inner.channels.get(channel_id).is_some_and(|c| !c.permanent)
        && inner.clients.in_channel(channel_id).is_empty()
        && inner.channels.children(channel_id).is_empty();
    if empty_temporary {
        delete_recursive(inner, channel_id);
    }
}

fn persist_channels(inner: &Inner) -> Result<(), Refusal> {
    inner
        .db
        .save_channels(&inner.channels.stored())
        .map_err(storage_refusal)
}

/// Gemeinsame Pruefung von Kick und Bann: darf moderieren, ist online, und
/// steht echt hoeher. `allow_self` fuer den Kick aus dem eigenen Kanal.
fn find_victim(
    inner: &Inner,
    by: &Client,
    target: u16,
    allow_self: bool,
) -> Result<Arc<Client>, Refusal> {
    if !Rights::may_moderate(by.role()) {
        return Err("Keine Berechtigung.".into());
    }
    let victim = inner
        .clients
        .get(target)
        .ok_or("Client ist nicht online.")?;
    let against_self = allow_self && victim.id() == by.id();
    if !(Rights::outranks(by.role(), victim.role()) || against_self) {
        return Err("Gleiche oder höhere Rolle.".into());
    }
    Ok(victim)
}

/// Sagt der Moderation, dass sich die Liste geaendert hat.
fn notify_approvals(inner: &Inner) {
    let message = Message::ApprovalsChanged {
        pending: inner.db.pending_approvals(),
    };
    for client in inner.clients.all() {
        if Rights::may_approve(client.role()) {
            client.send(message.clone());
        }
    }
}

/// Kodiert einmal und schickt allen, ausser `except`.
fn broadcast(inner: &Inner, message: &Message, except: Option<u16>) {
    let Ok(frame) = encode(&Envelope::event(message.clone())) else {
        return;
    };
    let frame = Frame::from(frame);
    for client in inner.clients.all() {
        if Some(client.id()) != except {
            client.send_frame(Arc::clone(&frame));
        }
    }
}

/// Wohin eine Nachricht gehoert (echo-182): in welches Archiv, und wer sie
/// bekommt, jeweils mit dem Archiv aus seiner Sicht. `None`, wenn es den
/// Empfaenger nicht mehr gibt.
struct Route {
    archive: Arc<ChannelArchive>,
    recipients: Vec<(Arc<Client>, ArchiveRef)>,
}

fn route(
    inner: &Inner,
    from: &Client,
    scope: ChatScope,
    channel: u64,
    target: u16,
) -> Option<Route> {
    match scope {
        ChatScope::Server => {
            let everyone = ArchiveRef {
                scope,
                channel: 0,
                peer: String::new(),
            };
            Some(Route {
                archive: Arc::clone(inner.archives.server()),
                recipients: inner
                    .clients
                    .all()
                    .into_iter()
                    .map(|c| (c, everyone.clone()))
                    .collect(),
            })
        }
        ChatScope::Channel => {
            let room = ArchiveRef {
                scope,
                channel,
                peer: String::new(),
            };
            Some(Route {
                archive: inner.archives.channel(channel).ok()?,
                recipients: inner
                    .clients
                    .in_channel(channel)
                    .into_iter()
                    .map(|c| (c, room.clone()))
                    .collect(),
            })
        }
        ChatScope::Private => {
            let to = inner.clients.get(target)?;
            let archive = inner
                .archives
                .dm(from.fingerprint(), to.fingerprint(), true)
                .ok()??;
            let mut recipients = vec![(
                Arc::clone(&to),
                ArchiveRef {
                    scope,
                    channel: 0,
                    peer: from.fingerprint().to_owned(),
                },
            )];
            if to.id() != from.id()
                && let Some(me) = inner.clients.get(from.id())
            {
                let peer = to.fingerprint().to_owned();
                recipients.push((
                    me,
                    ArchiveRef {
                        scope,
                        channel: 0,
                        peer,
                    },
                ));
            }
            Some(Route {
                archive,
                recipients,
            })
        }
    }
}

/// Haengt die Nachricht an ihr Archiv und verteilt sie mit ihrer Nummer. Der
/// Absender bekommt sie immer, auch wenn er inzwischen den Kanal gewechselt
/// hat: erst mit ihr weiss er, dass sie angekommen ist.
fn publish(route: &Route, from: &Client, text: &str) -> Result<(), StorageError> {
    let message = route.archive.append(
        ticks::unix_millis_now(),
        from.fingerprint(),
        &from.name(),
        text,
        None,
    )?;
    let dto = to_dto(&route.archive, &message);
    for (to, archive) in &route.recipients {
        to.send(Message::ChatEvent {
            archive: archive.clone(),
            from_id: from.id(),
            message: dto.clone(),
        });
    }
    let sender_included = route.recipients.iter().any(|(to, _)| to.id() == from.id());
    if let (false, Some((_, archive))) = (sender_included, route.recipients.first()) {
        from.send(Message::ChatEvent {
            archive: archive.clone(),
            from_id: from.id(),
            message: dto,
        });
    }
    Ok(())
}

fn to_dto(store: &ChannelArchive, message: &ArchivedMessage) -> ChatMessageDto {
    let image = message
        .image
        .as_deref()
        .and_then(|id| store.image(id).ok().flatten())
        .map(|image| ChatImageDto {
            id: image.id,
            width: u32::try_from(image.width).unwrap_or(0),
            height: u32::try_from(image.height).unwrap_or(0),
            size: image.size,
        });
    ChatMessageDto {
        seq: message.seq,
        time: message.time,
        author: message.author.clone(),
        author_name: message.author_name.clone(),
        text: message.text.clone(),
        image,
    }
}

/// Wer welches Archiv lesen darf: den Serverchat jeder, einen Kanal nur, wer
/// gerade darin ist, ein privates Gespraech nur die beiden. `None`, wenn es
/// das Gespraech noch nicht gibt.
fn readable(
    inner: &Inner,
    by: &Client,
    archive: &ArchiveRef,
) -> Result<Option<Arc<ChannelArchive>>, Refusal> {
    match archive.scope {
        ChatScope::Server => Ok(Some(Arc::clone(inner.archives.server()))),
        ChatScope::Channel => {
            if !inner.channels.exists(archive.channel) {
                return Err("Kanal existiert nicht.".into());
            }
            if by.channel() != archive.channel {
                return Err("Den Verlauf eines Kanals sieht nur, wer darin ist.".into());
            }
            inner
                .archives
                .channel(archive.channel)
                .map(Some)
                .map_err(storage_refusal)
        }
        ChatScope::Private => inner
            .archives
            .dm(by.fingerprint(), &archive.peer, false)
            .map_err(storage_refusal),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn storage_refusal(error: StorageError) -> Refusal {
    format!("Speicherfehler: {error}")
}
