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
  UTC), JSON in camelCase mit Enums als Text. Geprueft wird das mit
  Archiven aus dem C#-Server als Testdaten.
- **Kein JSONL.** Der Client legt Einstellungen, Favoriten und `known_hosts`
  in `%AppData%\echo\client.sqlite` ab, ueber turso wie der Server. Die
  JSONL-Dateien des C#-Clients werden nicht eingelesen; Favoriten und
  gemerkte Server legt man einmal neu an.
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
| QUIC, Datagramme | `quinn` ohne rustls-Features, `rustls` ohne Provider, dazu `echo-quic` | Datagramme nach RFC 9221. `echo-quic` bringt den TLS-Provider aus RustCrypto und die quinn-Anbindung selbst mit (echo-225); `rustls-rustcrypto` war unbrauchbar (QUIC nur `todo!()`, Luecken in Abhaengigkeiten) |
| Nachrichten | `serde`, `postcard`, `tokio-util` (LengthDelimitedCodec) | `WIRE_VERSION` im Handshake |
| Identitaet, Zertifikat | `p256`, `sha2`, `x509-cert` | ECDSA P-256 als PEM wie bisher, gleicher Fingerabdruck; kein `rcgen`, das `ring` zieht |
| TLS-Bausteine | `aes-gcm`, `aes`, `hmac`, `hkdf`, `x25519-dalek` | nur TLS 1.3 mit `TLS13_AES_128_GCM_SHA256`, X25519 und ECDSA P-256; geprueft gegen RFC 9001, Anhang A |
| Opus | `rusty-opus` | in-band FEC, PLC; Tests gegen die RFC-Vektoren |
| Rauschunterdrueckung | `nnnoiseless` | RNNoise in Rust |
| Audio-Ein-/Ausgabe | `wasapi`, sonst `windows` | inkl. Process Loopback (echo-218) |
| Resampling | `rubato` | |
| Musik dekodieren | `symphonia` | mp3, aac, flac, ogg, wav, mp4, mkv |
| AC-3, E-AC-3, DTS | eigener Port | aus `WowTs3.Core/Audio/Dolby` und `Dts` |
| Video im Netz | `rav1e` 0.8 (Encoder, nur `threading`) + `rav1d` 1.1 (Decoder, nur `bitdepth_8`, ueber seine `dav1d_*`-Funktionen mit eigenem safe Wrapper) | AV1, gemessen in echo-227: Bildschirm 1080p mit 15–20 fps als Standard, 720p30 fuer Bewegtes; ~133 ms feste Verzoegerung in rav1e (echo-233). Nicht `rav1d-safe` (AGPL) |
| H.264 aus Dateien | `rusty_h264` (nur Decoder, `default-features = false`) | bit-genau zu ffmpeg, bis 180 fps bei 1080p. Ohne `global-alloc`, das bremst rav1e um 30–45 %. Film geht als 720p24 ins Netz; 1080p schafft rav1e nicht in Echtzeit |
| Videodateien | `mp4`, `matroska-demuxer` | Bildrate aus Timescale und Sample-Dauern rechnen, `frame_rate()` rundet falsch; Ton ueber symphonia und die eigenen Decoder |
| YUV, Skalieren | `yuvutils-rs` | reines Rust, BGRA → I420 bei 1080p in 1 ms |
| Bildschirmaufnahme | `windows-capture` | Windows Graphics Capture in reinem Rust: Bildschirm, einzelne Fenster, Mauszeiger. Linux/macOS offen (echo-230) |
| SQLite, sqlar | `turso_core` ohne Default-Features (`fs`, `json`, `time`, `uuid`, `pure-rust-crypto`), `flate2` mit `miniz_oxide` | SQLite neu in Rust, gleiches Dateiformat, siehe unten |
| JSON | `serde_json` | Einstellungen des Servers und Formen der Tafeln liegen als JSON in den Archiven |
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
