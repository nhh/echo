# Sicherheit

## Eine Luecke melden

Bitte **kein oeffentliches Issue**. Nutze stattdessen die private Meldung
von GitHub: im Repository unter **Security → Report a vulnerability**.

Hilfreich sind:

- was betroffen ist (Server, Client, Protokoll, Archive),
- wie es sich ausloesen laesst,
- was ein Angreifer damit erreicht.

Du bekommst innerhalb einer Woche eine Antwort. Wir beheben die Luecke,
bevor sie oeffentlich wird, und nennen dich auf Wunsch im Hinweis zur
Behebung.

## Was unterstuetzt wird

echo ist noch in Arbeit und hat keine Releases. Bis zum ersten Release wird
nur der aktuelle Stand von `main` gepflegt.

## Was echo selbst tut

- Abhaengigkeiten werden in CI mit `cargo deny check advisories` gegen die
  RustSec-Datenbank geprueft; Dependabot schlaegt Updates woechentlich vor.
- Keine Bibliotheken in C, C++ oder Assembler, auch nicht fuer
  Verschluesselung (`cargo deny check bans`).
