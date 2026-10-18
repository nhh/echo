## Was und warum

<!-- Was aendert sich fuer Nutzer oder Entwickler, und warum? -->

Refs: #

## Geprueft

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo llvm-cov --workspace --fail-under-lines 60`
- [ ] `cargo deny check`
- [ ] Commit-Nachrichten nach [CONTRIBUTING.md](../CONTRIBUTING.md), jede mit `Refs:`
- [ ] Bei Aenderungen an der Oberflaeche: Screenshot aus `--shot` angehaengt
- [ ] Bei Aenderungen an Archiven oder Einstellungen: alte Daten bleiben lesbar
