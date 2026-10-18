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
  Dateinamen (`ArchiveKeys`), gleiche Zeitformate (Nachrichten, Dateien und
  Tafeln in Unix-Millisekunden, `sqlar.mtime` in Unix-Sekunden, die
  Server-Tabellen `members`, `bans`, `approvals` und `events` in .NET-Ticks
  UTC), JSON in camelCase mit Enums als Text. Dasselbe gilt fuer die JSONL-Dateien des
  Clients unter `%AppData%\echo`. Geprueft wird das mit Archiven aus dem
  C#-Server als Testdaten.
- **Opus in reinem Rust** hinter dem Trait `VoiceCodec`, damit das Crate
  tauschbar bleibt; libopus nur als Rueckfall.
- **Zeichnen ueber wgpu** nach dem Vorbild von `../picasso.net`
  (`picasso-gfx`, `picasso-ui`, Host in `crates/picasso/src/host`): eine
  Zeichenliste, abgespielt auf vello (GPU) oder vello_cpu (ohne GPU, fuer
  `--shot` und Tests); Text mit harfrust und skrifa. Kein Skia, kein OpenGL.
- **Reines Rust:** in die Binaries wird kein C, C++ oder Assembler
  kompiliert; kein Crate mit `cc`, `cmake` oder `bindgen` im Build. Alle
  Codecs (Opus, H.264, AC-3, DTS, mp3/aac/flac) sind Rust. Das
  Betriebssystem wird ueber das `windows`-Crate nur fuer Geraete gerufen:
  Audio-Ein- und -Ausgabe (WASAPI), Bildschirmaufnahme (DXGI), Fenster und
  GPU (ueber wgpu). Kein Media Foundation, kein ring, kein aws-lc-rs, kein
  libsqlite3, kein libopus, kein ffmpeg.
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
| H.264 | `rusty_h264` | Encoder und Decoder in Rust; erst Spike (echo-227), Rueckfall AV1 mit `rav1e` + `rav1d` |
| Videodateien | `mp4` bzw. `re_mp4`, `matroska-demuxer` | Ton daraus ueber symphonia und die eigenen Decoder |
| NV12, Skalieren | `yuvutils-rs` oder wgpu-Compute | |
| Bildschirmaufnahme | `windows` (DXGI Desktop Duplication) | einziger Video-Teil am Betriebssystem |
| SQLite, sqlar | `turso_core` ohne Default-Features (`fs`, `json`, `time`, `uuid`, `pure-rust-crypto`), `flate2` mit `miniz_oxide` | SQLite neu in Rust, gleiches Dateiformat, siehe unten |
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
| `echo-media` | Codecs und Demuxer in Rust; dazu WASAPI und DXGI-Aufnahme, nur unter Windows |
| `echo-ui` | Immediate-Mode-Widgets in Fluent 2 auf vello |
| `echo-client` | Die Exe: Fenster, Screens, `--shot`, `--selftest` |
| `echo-server` | QUIC-Listener, `ServerState`, SFU, Archive |
| `xtask` | `cargo xtask dist` |

## turso und die sqlar-Archive (echo-226)

Geprueft mit Archiven, die der C#-Server selbst geschrieben hat
(`tests/fixtures/csharp`, erzeugt mit `tools/gen-csharp-fixtures.cs`), und
gegengelesen mit dem echten SQLite:

- Lesen: alle Tabellen, `page_size=65536`, WAL, Unicode, Bilder, Dateien in
  16-MB-Stuecken ueber inkrementelles Blob-Lesen (`blob_open`), und ein
  nicht sauber geschlossenes Archiv, dessen Inhalt nur im `-wal` steht.
- Schreiben: Chat, `zeroblob` plus inkrementelles Blob-Schreiben,
  `INSERT OR REPLACE`, Loeschen nach Namensbereich, neues Archiv mit
  `page_size=65536` und WAL, `CHECK`-Bedingung. SQLite meldet danach
  `integrity_check = ok` und liest alles wie `sqlite3 -A`.
- Ein Leser sieht waehrend einer offenen Schreibtransaktion den alten Stand.

Was dabei zu beachten ist:

- **Nicht das Crate `turso` nehmen**, sondern `turso_core`: `turso` zieht C
  mit (`mimalloc`, `simsimd`, `aegis`, `bindgen`). `turso_core` braucht ohne
  Default-Features ausdruecklich `uuid`, sonst baut es nicht. `cc` bleibt als
  Build-Abhaengigkeit von `aegis` im Baum, kompiliert mit `pure-rust` aber
  nichts.
- **Lange Pfade unter Windows:** turso oeffnet keine Pfade ueber 260
  Zeichen. Die Namen der DM-Archive sind 2 x 64 Hexziffern lang; Pfade gehen
  darum immer als `\\?\`-Pfad an turso.
- **Kein zweiter Prozess:** turso legt keine `-shm`-Datei an. Ein
  `sqlite3 -A` darf ein Archiv nur lesen, wenn der Server es nicht offen hat.
