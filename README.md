# echo

Sprachchat mit eigenem Server und Client, in Rust: der Client als **eine
einzelne Windows-Exe**, gezeichnet mit wgpu in Fluent 2; der Server als
Konsole fuer Windows und Linux. Port des C#-Projekts
[nhh/wowts3](https://github.com/nhh/wowts3) (echo-224).

Die Regeln fuer Code, Tests und Commits stehen in [CLAUDE.md](CLAUDE.md).

## Entscheidungen

- **Server und Client sind beide Rust.** Das Netzformat muss nicht zum
  C#-Server passen; es wird neu und mit serde/postcard beschrieben. Ein
  Rust-Client spricht nur mit einem Rust-Server.
- **Die Daten bleiben lesbar.** Der Rust-Server oeffnet die vorhandenen
  `.sqlar`-Archive des C#-Servers ohne Umwandlung: gleiche Tabellen
  (`sqlar`, `messages`, `images`, `files`, `boards`, `board_shapes`,
  `settings`, `channels`, `members`, `bans`, `approvals`, `events`), gleiche
  Dateinamen (`ArchiveKeys`), Zeiten als Unix-Millisekunden, JSON in
  camelCase mit Enums als Text. Dasselbe gilt fuer die JSONL-Dateien des
  Clients unter `%AppData%\echo`. Geprueft wird das mit Archiven aus dem
  C#-Server als Testdaten.
- **Opus in reinem Rust** hinter dem Trait `VoiceCodec`, damit das Crate
  tauschbar bleibt; libopus nur als Rueckfall.
- **Zeichnen ueber wgpu** nach dem Vorbild von `../picasso.net`
  (`picasso-gfx`, `picasso-ui`, Host in `crates/picasso/src/host`): eine
  Zeichenliste, abgespielt auf vello (GPU) oder vello_cpu (ohne GPU, fuer
  `--shot` und Tests); Text mit harfrust und skrifa. Kein Skia, kein OpenGL.
- **Reines Rust:** in die Binaries wird kein C, C++ oder Assembler
  kompiliert; kein Crate mit `cc`, `cmake` oder `bindgen` im Build. Erlaubt
  sind Aufrufe von Betriebssystem-APIs ueber das `windows`-Crate (WASAPI,
  Media Foundation, D3D, Win32) und was wgpu vom System nutzt. Darum kein
  `ring`, kein `aws-lc-rs`, kein libsqlite3, kein libopus, kein ffmpeg.
- **Eine Datei:** nichts wird zur Laufzeit ausgepackt oder nachgeladen.

## Techstack

| Bereich | Crate | Anmerkung |
|---|---|---|
| Async | `tokio` | |
| QUIC, Datagramme | `quinn` (ohne `ring`), `rustls` mit `rustls-rustcrypto` | Datagramme nach RFC 9221; Provider ist experimentell, erst Spike |
| Nachrichten | `serde`, `postcard`, `tokio-util` (LengthDelimitedCodec) | `WIRE_VERSION` im Handshake |
| Identitaet, Zertifikat | `p256`, `ecdsa`, `sha2`, `x509-cert` | ECDSA P-256 als PEM wie bisher; kein `rcgen`, das `ring` zieht |
| Opus | `rusty-opus` | in-band FEC, PLC; Tests gegen die RFC-Vektoren |
| Rauschunterdrueckung | `nnnoiseless` | RNNoise in Rust |
| Audio-Ein-/Ausgabe | `wasapi`, sonst `windows` | inkl. Process Loopback (echo-218) |
| Resampling | `rubato` | |
| Musik dekodieren | `symphonia` | mp3, aac, flac, ogg, wav, mp4, mkv |
| AC-3, E-AC-3, DTS | eigener Port | aus `WowTs3.Core/Audio/Dolby` und `Dts` |
| Video | `windows` (Media Foundation, D3D11, DXGI) | H.264 in Hardware, Desktop Duplication |
| SQLite, sqlar | `turso` (SQLite neu in Rust, Beta), `flate2` mit `miniz_oxide` | Dateiformat von SQLite; erst Spike gegen C#-Archive |
| JSON, JSONL | `serde_json` | |
| Fenster | `winit` 0.30 | Hauptfenster, Toast, Randleiste, Vollbild; Host wie in picasso.net |
| GPU | `wgpu` ueber `vello` 0.10 | D3D12 unter Windows |
| 2D-Zeichnen | `vello`, `vello_cpu`, `kurbo`, `peniko` | Zeichenliste mit zwei Backends wie `picasso-gfx`; `--shot` ueber `vello_cpu` |
| Text | `harfrust`, `skrifa` | Formung wie HarfBuzz; Schriften eingebettet |
| Widgets | Fluent 2 wie `picasso-ui` | dort ist skiasilk schon nach Rust portiert |
| Bilder | `image` | PNG, JPEG, GIF/WebP animiert, BMP |
| Tray, Dialog, Zwischenablage | `tray-icon`, `rfd`, `arboard` | Push-to-Talk ueber `GetAsyncKeyState` |
| Serverzustand | `parking_lot`, `arc-swap` | |
| CLI, Logs, Fehler | `clap`, `tracing`, `thiserror`, `anyhow` | |
| Tests | `cargo-nextest`, `cargo-llvm-cov`, `proptest`, `insta` | Zeilenabdeckung mindestens 60 % |

## Aufbau (geplant)

| Crate | Inhalt |
|---|---|
| `echo-protocol` | Nachrichten, Framing, Sprachframe, Identitaet, Host-Zertifikat, `VoiceCodec`. Keine IO |
| `echo-storage` | sqlar-Archive, Kanalarchiv, JSONL |
| `echo-core` | Session, Jitter-Puffer, Mixer, Sprachaktivierung, Decoder, Whiteboard, Text. Plattformfrei |
| `echo-media` | WASAPI, Media Foundation, D3D11. Nur Windows |
| `echo-ui` | Immediate-Mode-Widgets in Fluent 2 auf vello |
| `echo-client` | Die Exe: Fenster, Screens, `--shot`, `--selftest` |
| `echo-server` | QUIC-Listener, `ServerState`, SFU, Archive |
| `xtask` | `cargo xtask dist` |
