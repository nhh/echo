# echo

[![CI](https://github.com/nhh/echo/actions/workflows/ci.yml/badge.svg)](https://github.com/nhh/echo/actions/workflows/ci.yml)
[![Lizenz: MIT](https://img.shields.io/badge/Lizenz-MIT-blue.svg)](LICENSE)
[![Reines Rust](https://img.shields.io/badge/reines-Rust-orange.svg)](docs/techstack.md)

Sprachchat mit eigenem Server und Client, ganz in Rust: Serverbaum,
Kanaele, Chat mit Bildern, Dateien, Tafeln, Sprache, Bildschirm teilen und
gemeinsam Filme schauen. Der Client ist **eine einzelne Exe** fuer Windows,
gezeichnet mit wgpu in Fluent 2; der Server ist ein Konsolenprogramm fuer
Windows und Linux.

echo ist ein unabhaengiges Projekt ohne Verbindung zu TeamSpeak Systems.

> **Stand:** in Arbeit. echo gibt es bereits als .NET-Anwendung; dieses
> Repository portiert sie nach Rust. Fertig sind bisher QUIC mit eigenem
> TLS-Provider (`echo-quic`) und die Grundlagen des Speichers (`echo-storage`). Was als Naechstes kommt, steht in
> [docs/techstack.md](docs/techstack.md).

## Was echo ausmacht

- **Eigenes Protokoll ueber QUIC:** ein Steuerstream fuer Nachrichten, ein
  QUIC-Datagramm je 20-ms-Sprachframe. Der Server leitet nur weiter (SFU),
  gemischt wird im Client.
- **Video in AV1, von echo selbst kodiert und dekodiert:** wer echo hat,
  sieht jeden Stream, egal was das System an Codecs mitbringt.
- **Identitaet wie bei SSH:** ein ECDSA-Schluessel je Nutzer, der
  Fingerabdruck ist die Nutzer-ID; der Server wird beim ersten Verbinden
  gemerkt und danach geprueft.
- **Daten in SQLite-Archiven** im Standardformat
  [sqlar](https://sqlite.org/sqlar.html): je Kanal eine Datei mit Chat,
  Bildern und Dateien, lesbar mit `sqlite3 -A`.
- **Reines Rust:** kein C, kein C++, kein Assembler, kein ffmpeg. Codecs
  (Opus, AV1, AC-3, DTS) sind Rust; das Betriebssystem wird nur fuer
  Geraete gerufen (Audio, Bildschirm, Fenster, GPU).

## Bauen

Voraussetzung ist Rust; die Version steht in `rust-toolchain.toml` und wird
von `rustup` von selbst geholt.

```
cargo build --workspace
cargo test --workspace
```

Vor einem Commit laeuft dasselbe wie in CI:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo llvm-cov --workspace --fail-under-lines 60
cargo deny check
```

## Aufbau

| Crate | Inhalt | Stand |
|---|---|---|
| `echo-storage` | SQLite: sqlar-Archive kompatibel zum .NET-Server, Datei des Clients | begonnen |
| `echo-quic` | QUIC mit Datagrammen ueber quinn, TLS-Provider in reinem Rust, Zertifikat des Servers | steht |
| `echo-protocol` | Nachrichten, Framing, Sprachframe, Identitaet, Zertifikat. Keine IO | geplant |
| `echo-core` | Session, Jitter-Puffer, Mixer, Sprachaktivierung, Decoder, Tafel | geplant |
| `echo-media` | Codecs und Demuxer in Rust, Audio-Geraete, Bildschirmaufnahme | geplant |
| `echo-ui` | Immediate-Mode-Widgets in Fluent 2 auf vello | geplant |
| `echo-client` | Die Exe: Fenster, Screens, `--shot`, `--selftest` | geplant |
| `echo-server` | QUIC-Listener, Serverzustand, SFU, Archive | geplant |

Mehr dazu:

- [docs/techstack.md](docs/techstack.md): Entscheidungen und Crates
- [docs/storage.md](docs/storage.md): was turso mit den Archiven kann und was nicht
- [CLAUDE.md](CLAUDE.md): Regeln fuer Code, Tests und Commits, fuer Menschen
  und Agenten

## Mitmachen

Beitraege sind willkommen. Bitte vorher [CONTRIBUTING.md](CONTRIBUTING.md)
lesen; fuer alle gilt der [Verhaltenskodex](CODE_OF_CONDUCT.md).
Sicherheitsluecken bitte nicht als Issue melden, sondern wie in
[SECURITY.md](SECURITY.md) beschrieben.

## Lizenz

[MIT](LICENSE)
