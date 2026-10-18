# Techstack und Entscheidungen

Was der Rust-Port von echo benutzt und warum (echo-224).

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
- **Zeichnen ueber wgpu** nach dem Vorbild von picasso.net, einem anderen
  Projekt des Maintainers, das denselben Weg schon gegangen ist: eine
  Zeichenliste, abgespielt auf vello (GPU) oder vello_cpu (ohne GPU, fuer
  `--shot` und Tests); Text mit harfrust und skrifa. Kein Skia, kein OpenGL.
- **Reines Rust:** in die Binaries wird kein C, C++ oder Assembler
  kompiliert; kein Crate mit `cc`, `cmake` oder `bindgen` im Build. Alle
  Codecs (Opus, AV1, H.264, AC-3, DTS, mp3/aac/flac) sind Rust. Das
  Betriebssystem wird ueber das `windows`-Crate nur fuer Geraete gerufen:
  Audio-Ein- und -Ausgabe (WASAPI), Bildschirmaufnahme (Windows Graphics
  Capture ueber `windows-capture`), Fenster und
  GPU (ueber wgpu). Kein Media Foundation, kein ring, kein aws-lc-rs, kein
  libsqlite3, kein libopus, kein ffmpeg.
- **Eine Datei:** nichts wird zur Laufzeit ausgepackt oder nachgeladen.
- **Video ist AV1, kodiert und dekodiert von echo selbst** (rav1e, rav1d) und
  gezeichnet ueber wgpu. Der Empfaenger braucht keinen Codec und keine
  Hardware-Unterstuetzung vom System: wer echo hat, sieht jeden Stream, auf
  jeder Plattform. H.264 wird nur noch dekodiert, um Filmdateien zu lesen.

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
| Video im Netz | `rav1e` (Encoder) + `rav1d` bzw. `rav1d-safe` (Decoder), beide ohne Feature `asm` | AV1 fuer Bildschirm und Film; Echtzeit mit `low_latency`, Geschwindigkeit erst messen (echo-227) |
| H.264 aus Dateien | `rusty_h264` oder `rust_h264` (nur Decoder) | nur beim Sender, um mp4/mov/mkv zu lesen; ins Netz geht AV1 |
| Videodateien | `mp4` bzw. `re_mp4`, `matroska-demuxer` | Ton daraus ueber symphonia und die eigenen Decoder |
| NV12, Skalieren | `yuvutils-rs` oder wgpu-Compute | |
| Bildschirmaufnahme | `windows-capture` | Windows Graphics Capture in reinem Rust: Bildschirm, einzelne Fenster, Mauszeiger. Linux/macOS offen (echo-230) |
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
| Tests | `cargo-llvm-cov`, `proptest`, `insta`, `tempfile` | Zeilenabdeckung mindestens 60 % |
