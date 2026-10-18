# CLAUDE.md — Arbeitsregeln fuer echo (Rust)

Dieses Repository ist der Port von echo nach Rust: Sprachchat mit eigenem
Server und Client, der Client als **eine einzelne Windows-Exe**, der Server
als Konsole fuer Windows und Linux. Das Vorbild ist das C#-Repository
`../wowts3` (github.com/nhh/wowts3, nicht oeffentlich; wie `../picasso.net`
nur fuer Maintainer mit Zugriff). Was dort steht, ist die Referenz fuer
Verhalten und Aussehen; hier wird es nicht 1:1 abgeschrieben, sondern in
idiomatischem Rust neu gebaut.

## 1. Vor jeder Aenderung

- Erst diese Datei, `README.md` und `docs/` lesen, dann den betroffenen Crate.
- Das Repository ist oeffentlich (MIT). Nichts Internes hinein: keine
  Zugangsdaten, keine Hostnamen von Servern, keine Kundendaten.
- Bei Fragen zum gewollten Verhalten im C#-Code nachsehen
  (`../wowts3/src/...`), nicht raten. `../wowts3` wird nicht veraendert.
- Fuer Fenster, wgpu, 2D-Zeichnen, Text und Widgets ist `../picasso.net`
  das Vorbild (`crates/picasso-gfx`, `crates/picasso-ui`,
  `crates/picasso/src/host`): dort ist derselbe Weg in Rust schon gegangen.
  Auch `../picasso.net` wird von hier aus nicht veraendert.
- Jede Aenderung endet mit:
  ```
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo llvm-cov --workspace --fail-under-lines 60
  cargo deny check
  ```
  UI-Aenderungen zusaetzlich mit `cargo run -p echo-client -- --shot out.png`
  und einem Blick auf das Bild.

## 2. Testabdeckung

- **Mindestens 60 % Zeilenabdeckung** ueber den ganzen Workspace, gemessen mit
  `cargo llvm-cov`. CI bricht darunter ab. Die Grenze wird nie gesenkt.
- Nichts wird pauschal aus der Messung genommen. Plattform-Code (WASAPI,
  Bildschirmaufnahme, Fenster) bleibt duenn: Logik gehoert in
  plattformfreie Funktionen hinter Traits, die sich ohne Geraet testen lassen.
- Getestet wird alles, was ohne Geraet geht: Protokoll-Roundtrips, Framing,
  Identitaet, Archive, Jitter-Puffer, Mixer, Pegel, Sprachaktivierung,
  Decoder (AC-3/E-AC-3/DTS gegen Referenz-PCM), Whiteboard-Dokument,
  Textumbruch, Server in-process mit zwei Clients ueber echtes QUIC auf
  localhost.
- Fuer Codecs und Parser `proptest`; fuer Ausgaben mit vielen Feldern
  `insta`-Snapshots. UI wird ueber `--shot` geprueft, nicht ueber
  Unit-Tests auf Zeichenaufrufe.
- Tests liegen als `#[cfg(test)] mod tests` neben dem Code; Tests ueber
  mehrere Crates (Server + Client) unter `tests/` des jeweiligen Crates.

## 3. Clean Code

- **Eine Verantwortung je Typ und je Crate.** `echo-core` kennt weder Skia
  noch Fenster; `echo-ui` kennt kein QUIC; `echo-protocol` macht keine IO.
- **Kleine Funktionen mit sprechendem Namen.** Wenn ein Kommentar einen Block
  erklaert, wird der Block eine Funktion mit dem Kommentar als Namen.
- **Keine Magie.** Jede Zahl im Layout ist ein benanntes Token, jeder Timeout
  eine benannte `const` vom Typ `Duration`.
- **Fehler sind Werte.** Bibliotheken liefern `Result` mit eigenen
  `thiserror`-Typen; `anyhow` nur in den Binaries. Netzwerk- und
  Audiofehler erscheinen in der UI als Toast. Kein `unwrap()`/`expect()`
  ausserhalb von Tests, ausser mit Kommentar, warum es nicht scheitern kann.
  Kein `panic!` im Frame-Loop.
- **Immutable, wo moeglich.** Was die UI liest (Snapshot des Serverbaums,
  Nachrichten), ist ein unveraenderlicher Wert, geteilt ueber `Arc`.
  Veraenderlicher Zustand hat genau einen Eigentuemer.
- **`unsafe` nur in Plattform-Modulen** (`windows`-Aufrufe), je Block ein
  `// SAFETY:`-Kommentar. `#![forbid(unsafe_code)]` in allen anderen Crates.
- **Kein toter Code**, keine auskommentierten Bloecke, kein „fuer spaeter“.
- **Kommentare erklaeren warum, nicht was.** `///` am Typ sagt, wofuer er da
  ist und was er nicht tut; Inline-Kommentare markieren Entscheidungen und
  Fallstricke. Ticketnummern (`echo-<n>`) duerfen im Kommentar stehen, wenn
  sie eine Entscheidung begruenden.

## 4. Threads und Nebenlaeufigkeit

- Die UI laeuft auf **einem** Thread als Immediate-Mode-Frame-Schleife und
  blockiert nie auf Netzwerk oder Audio.
- Netzwerk laeuft auf `tokio`. Ergebnisse kommen als Events ueber einen
  Kanal in die UI, die ihn je Frame leert.
- Auf dem Server ist `ServerState` die einzige Stelle mit veraenderlichem
  Zustand, jede Methode eine atomare Aenderung unter einem Mutex. Sprachframes
  gehen daran vorbei ueber einen unveraenderlichen Snapshot (`ArcSwap`).
- Audio-Threads sind Echtzeit: keine Allokation, kein Lock mit
  Wartezeit, kein `await`. Sie reden mit dem Rest nur ueber lockfreie
  Ringpuffer und Atomics.

## 5. Stil

- Formatierung ist `rustfmt` (Standard), Lints sind `clippy` mit
  `-D warnings`; Abweichungen stehen in `[workspace.lints]` mit Begruendung.
- Edition 2024, die Toolchain steht in `rust-toolchain.toml`.
- Abhaengigkeiten nur ueber `[workspace.dependencies]`; jede neue Crate
  braucht einen Grund im Commit-Body.
- **Reines Rust.** Kein C, C++ oder Assembler im Build: kein Crate, das
  `cc`, `cmake` oder `bindgen` braucht, kein `ring`, kein `aws-lc-rs`, kein
  `-sys`-Crate mit mitgebrachten Quellen. Codecs sind immer Rust; das
  `windows`-Crate nur fuer Geraete (WASAPI, Bildschirmaufnahme, Fenster), nie
  fuer Media Foundation. CI prueft das mit `scripts/check-pure-rust.sh`;
  einzige Ausnahme ist `cc` unter `aegis`, das mit `pure-rust` nichts baut. Keine DLL
  wird mitgeliefert oder nachgeladen: die Exe bleibt eine Datei.
- Code und Bezeichner Englisch, Kommentare und UI-Texte Deutsch
  (ae/oe/ue/ss in Kommentaren, echte Umlaute in UI-Texten).

## 6. Protokoll

- Jede Aenderung an den Nachrichtentypen in `echo-protocol` erhoeht
  `WIRE_VERSION`. Der Versionsvergleich passiert im Handshake, damit der
  Nutzer einen lesbaren Grund bekommt; die ALPN-Kennung bleibt versionslos.
- Server und Client werden zusammen ausgeliefert.
- **Die Datenformate auf der Platte sind nicht frei.** `.sqlar`-Archive des
  Servers und JSONL-Dateien des Clients bleiben mit dem C#-Vorgaenger
  kompatibel (Tabellen, Spalten, Dateinamen, Zeitformate, JSON in camelCase,
  Enums als Text; Einzelheiten in `docs/techstack.md` und `docs/storage.md`). `tests/fixtures/csharp`
  enthaelt Archive des C#-Servers; jede Aenderung am Storage wird dagegen
  getestet. Eine Aenderung daran braucht ein Ticket, eine
  Migration und `BREAKING CHANGE:`.

## 7. Conventional Commits

Jede Commit-Nachricht folgt <https://www.conventionalcommits.org/> 1.0.0:

```
<typ>(<bereich>): <kurzbeschreibung>

<optionaler Body: was und warum, nicht wie>

Refs: echo-<n>
```

**Typen** (nur diese): `feat`, `fix`, `refactor`, `perf`, `test`, `docs`,
`build`, `ci`, `chore`, `style`.

**Bereiche** (optional): `protocol`, `storage`, `core`, `audio`, `video`,
`session`, `server`, `ui`, `client`, `board`, `build`, `ci`, `deps`.

- **Jeder Commit nennt sein Ticket im Footer: `Refs: echo-<n>`**, auch
  Zwischenschritte und Tests. Ohne Ticket kein Commit. Beitraege von aussen
  nennen ihr GitHub-Issue (`Refs: #<n>`). `scripts/check-commits.sh` prueft
  das in CI.
- Kurzbeschreibung Englisch, Imperativ, klein, ohne Punkt, max. 72 Zeichen.
- Ein Commit = eine logische Aenderung. Refactoring und Feature nie mischen.
- `!` bzw. `BREAKING CHANGE:` bei inkompatiblen Aenderungen an
  Einstellungsdateien, CLI-Optionen oder Archivformat.
- Nach jedem Commit sind fmt, clippy und Tests inklusive Abdeckung gruen.

## 8. Tickets in YouTrack

Tickets liegen im YouTrack-Projekt **echo** (`echo-<n>`), dasselbe wie fuer
das C#-Repository.

- Summary beginnt mit `Bug:`, `Feature:` oder `Task:`, danach wie eine
  Commit-Kurzbeschreibung.
- Stage `Backlog` beim Anlegen, `Develop` waehrend der Arbeit, `Done` mit dem
  erledigenden Commit. Priority `Normal`, `Major` nur fuer Blocker.
- Was beim Arbeiten auffaellt und nicht sofort behoben wird, wird ein Ticket.
