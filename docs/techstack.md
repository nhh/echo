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
  Codecs (Opus, H.264, AC-3, DTS, mp3/aac/flac) sind Rust. Das
  Betriebssystem wird ueber das `windows`-Crate nur fuer Geraete gerufen:
  Audio-Ein- und -Ausgabe (WASAPI), Bildschirmaufnahme (Windows Graphics
  Capture ueber `windows-capture`), Fenster und
  GPU (ueber wgpu). Kein Media Foundation, kein ring, kein aws-lc-rs, kein
  libsqlite3, kein libopus, kein ffmpeg.
- **Eine Datei:** nichts wird zur Laufzeit ausgepackt oder nachgeladen.
- **Video kodiert und dekodiert echo selbst** und zeichnet es ueber wgpu:
  der Empfaenger braucht keinen Codec vom System. **Welcher Codec, ist
  zurueckgestellt** (2026-10-18): AV1 in reinem Rust reicht nicht, siehe
  unten. H.264 wird dekodiert, um Filmdateien zu lesen.

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
| Video im Netz | zurueckgestellt | AV1 (rav1e + rav1d) gemessen und vorerst verworfen, siehe Abschnitt unten; H.265 hat in Rust nur Decoder |
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

## Video-Codec: Messung und Entscheidung (echo-227)

AV1 in reinem Rust, ohne Assembler, auf einem i9-14900KF (24 Kerne), rav1e
0.8.1 mit speed 10 und `low_latency`, rav1d 1.1.0:

| Fall | Ergebnis |
|---|---|
| Bildschirm 720p30 | haelt mit Reserve |
| Bildschirm 1080p30 | haelt knapp, einzelne Frames zu spaet, ~10 Kerne belegt |
| Bildschirm 1440p | ~20 fps |
| Film 1080p24 (H.264 → AV1) | 15–23 fps, **nicht in Echtzeit** |
| Film 720p24 | 33 fps, mit Reserve |
| Dekodieren (rav1d) | unkritisch, 1080p 400+ fps |
| Latenz | ~160 ms bei 30 fps, davon ~133 ms feste Verzoegerung in rav1e (echo-233) |

Fallen: rav1d 1.x hat nur die C-ABI (`dav1d_*`), aus Rust aufrufbar, aber
ohne safe API; `rav1d-safe` ist AGPL. rusty_h264 setzt mit seinem
Default-Feature `global-alloc` einen globalen Allocator, der rav1e um
30–45 % bremst.

**Entscheidung (2026-10-18): AV1 ist zurueckgestellt.** Video kommt erst,
wenn ein Codec in reinem Rust die Faelle oben schafft oder die Regel fuer
Video anders entschieden wird. H.265 ist keine Abkuerzung: in Rust gibt es
reife Decoder (`rusty_h265`, `rust_h265`), aber keinen brauchbaren Encoder.

