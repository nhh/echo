# Archive mit turso

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
