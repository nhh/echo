//! Wer darf was. Eine Regel, eine Funktion, ein Grund — und beide Seiten
//! lesen dieselbe: der Server erzwingt sie, der Client graut aus, was der
//! Server ohnehin ablehnen wuerde (echo-32).
//!
//! Rangfolge Gast < Mitglied < Admin < Besitzer. Fuer Handlungen gegen
//! andere gilt durchgehend [`Rights::outranks`]: gleiche oder hoehere Rolle
//! ist tabu, sonst koennten sich zwei Admins gegenseitig hinauswerfen.

use crate::Role;

#[derive(Debug)]
pub struct Rights;

impl Rights {
    /// Gegen jemanden handeln (werfen, bannen, Rolle setzen) darf nur, wer
    /// echt hoeher steht.
    #[must_use]
    pub fn outranks(actor: Role, target: Role) -> bool {
        actor > target
    }

    /// Moderieren: werfen, bannen, andere verschieben.
    #[must_use]
    pub fn may_moderate(role: Role) -> bool {
        role >= Role::Admin
    }

    /// Admins kommen ueberall hin, damit ein Server nicht an einem
    /// vergessenen Kanalpasswort zerfaellt.
    #[must_use]
    pub fn may_bypass_channel_password(role: Role) -> bool {
        role >= Role::Admin
    }

    /// Anklopfende zulassen oder abweisen (echo-39).
    #[must_use]
    pub fn may_approve(role: Role) -> bool {
        role >= Role::Admin
    }

    /// Temporaere Kanaele darf jedes Mitglied anlegen, dauerhafte nur ein
    /// Admin: sie ueberleben den letzten Nutzer.
    #[must_use]
    pub fn may_create_channel(role: Role, permanent: bool) -> bool {
        if permanent {
            role >= Role::Admin
        } else {
            role >= Role::Member
        }
    }

    /// Den eigenen temporaeren Kanal raeumt der Ersteller selbst weg.
    #[must_use]
    pub fn may_delete_channel(role: Role, own_temporary: bool) -> bool {
        own_temporary || role >= Role::Admin
    }

    /// Dieselbe Regel wie beim Loeschen (echo-30).
    #[must_use]
    pub fn may_edit_channel(role: Role, own_temporary: bool) -> bool {
        own_temporary || role >= Role::Admin
    }

    /// Andere verschieben; sich selbst darf jeder bewegen.
    #[must_use]
    pub fn may_move_others(role: Role) -> bool {
        Self::may_moderate(role)
    }

    /// Der Besitzer vergibt jede Rolle, ein Admin nur unterhalb von Admin —
    /// sonst koennte er sich Gleichrangige schaffen.
    #[must_use]
    pub fn may_assign_role(actor: Role, new_role: Role) -> bool {
        actor == Role::Owner || (actor >= Role::Admin && new_role < Role::Admin)
    }

    /// Dateien in einen Kanal legen (echo-20).
    #[must_use]
    pub fn may_upload(role: Role) -> bool {
        role >= Role::Member
    }

    /// Fremde Dateien loeschen; die eigenen darf jeder Hochladende entfernen.
    #[must_use]
    pub fn may_delete_foreign_files(role: Role) -> bool {
        Self::may_moderate(role)
    }

    /// Chatbilder liegen auf dem Server wie Dateien (echo-166).
    #[must_use]
    pub fn may_send_images(role: Role) -> bool {
        Self::may_upload(role)
    }

    /// Musik, Film oder Bildschirm in den Kanal senden (echo-16, echo-18).
    #[must_use]
    pub fn may_stream(role: Role) -> bool {
        role >= Role::Member
    }

    /// Banner fuer Server und Kanaele setzen (echo-23).
    #[must_use]
    pub fn may_set_banner(role: Role, own_channel: bool) -> bool {
        own_channel || role >= Role::Admin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Role::{Admin, Guest, Member, Owner};

    #[test]
    fn nobody_acts_against_equals() {
        assert!(Rights::outranks(Owner, Admin));
        assert!(!Rights::outranks(Admin, Admin));
        assert!(!Rights::outranks(Member, Admin));
    }

    #[test]
    fn admins_moderate_members_do_not() {
        for (role, may) in [
            (Guest, false),
            (Member, false),
            (Admin, true),
            (Owner, true),
        ] {
            assert_eq!(Rights::may_moderate(role), may);
            assert_eq!(Rights::may_move_others(role), may);
            assert_eq!(Rights::may_delete_foreign_files(role), may);
            assert_eq!(Rights::may_bypass_channel_password(role), may);
            assert_eq!(Rights::may_approve(role), may);
        }
    }

    #[test]
    fn channels_follow_permanence_and_ownership() {
        assert!(Rights::may_create_channel(Member, false));
        assert!(!Rights::may_create_channel(Member, true));
        assert!(!Rights::may_create_channel(Guest, false));
        assert!(Rights::may_create_channel(Admin, true));
        assert!(Rights::may_delete_channel(Guest, true));
        assert!(!Rights::may_delete_channel(Member, false));
        assert!(Rights::may_edit_channel(Member, true));
        assert!(Rights::may_edit_channel(Admin, false));
    }

    #[test]
    fn admins_cannot_create_their_equals() {
        assert!(Rights::may_assign_role(Owner, Owner));
        assert!(Rights::may_assign_role(Admin, Member));
        assert!(!Rights::may_assign_role(Admin, Admin));
        assert!(!Rights::may_assign_role(Member, Guest));
    }

    #[test]
    fn guests_only_listen() {
        assert!(!Rights::may_upload(Guest));
        assert!(!Rights::may_send_images(Guest));
        assert!(!Rights::may_stream(Guest));
        assert!(Rights::may_stream(Member));
        assert!(Rights::may_set_banner(Guest, true));
        assert!(!Rights::may_set_banner(Member, false));
        assert!(Rights::may_set_banner(Admin, false));
    }
}
