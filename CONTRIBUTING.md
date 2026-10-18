# Mitmachen bei echo

Danke, dass du helfen willst. Diese Seite sagt, wie ein Beitrag aussieht,
damit er schnell angenommen werden kann. Die vollstaendigen Regeln fuer Code,
Tests und Commits stehen in [CLAUDE.md](CLAUDE.md); sie gelten fuer
Menschen und Agenten gleich.

## Bevor du anfaengst

- **Fehler:** ein Issue mit der Vorlage „Fehler“ anlegen: was du getan hast,
  was du erwartet hast, was passiert ist.
- **Neue Faehigkeit:** erst ein Issue mit der Vorlage „Wunsch“, damit wir
  klaeren, ob und wie es passt, bevor du Zeit hineinsteckst.
- Kleine Korrekturen (Tippfehler, Doku) gehen direkt als Pull Request.

## Der Pull Request

1. Fork, Branch von `main`, zum Beispiel `fix/123-kurzer-name`.
2. Aendern, mit Tests. Die Zeilenabdeckung des Workspace bleibt bei
   mindestens 60 %.
3. Lokal pruefen, genau wie CI:
   ```
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo llvm-cov --workspace --fail-under-lines 60
   cargo deny check
   ```
   `cargo-llvm-cov` und `cargo-deny` installierst du mit
   `cargo install cargo-llvm-cov cargo-deny`.
4. Pull Request gegen `main` mit der Vorlage ausfuellen.

## Commit-Nachrichten

[Conventional Commits](https://www.conventionalcommits.org/) mit Ticket im
Footer; CI prueft das (`scripts/check-commits.sh`):

```
fix(storage): keep the image when a message is edited

Refs: #42
```

- Typen: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `ci`,
  `chore`, `style`.
- Kurzbeschreibung Englisch, Imperativ, klein, ohne Punkt, hoechstens 72
  Zeichen.
- Footer `Refs: #<issue>` mit dem GitHub-Issue. Die Maintainer fuehren ihre
  Tickets in YouTrack und schreiben dort `Refs: echo-<n>`.
- Ein Commit ist eine logische Aenderung; Umbau und neue Faehigkeit nie
  zusammen.

## Was nicht angenommen wird

- **Code in C, C++ oder Assembler**, auch nicht ueber ein Crate. echo ist
  reines Rust (`cargo deny check bans` prueft das).
- Abhaengigkeiten ohne Grund. Jede neue Crate braucht eine Begruendung im
  Commit-Body und eine Lizenz, die zu MIT passt.
- Aenderungen am Format der Archive ohne Migration. Die Daten bestehender
  Server muessen lesbar bleiben.

## Sprache

Code und Bezeichner sind Englisch, Kommentare, Doku und Texte in der
Oberflaeche Deutsch. Issues und Pull Requests duerfen Deutsch oder Englisch
sein.

## Lizenz

Mit einem Beitrag stimmst du zu, dass er unter der [MIT-Lizenz](LICENSE)
des Projekts steht.
