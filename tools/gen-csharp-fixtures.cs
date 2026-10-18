#:project ../../wowts3/src/WowTs3.Server/WowTs3.Server.csproj
#:property TargetFramework=net10.0
#:property RuntimeIdentifier=win-x64
#:property SelfContained=true
#:property PublishAot=false

// Erzeugt Archive mit dem echten C#-Code von echo als Testdaten fuer den
// Rust-Port (echo-226). Braucht den C#-Vorgaenger (nicht oeffentlich) in
// ../wowts3 neben diesem Repository; die erzeugten Archive liegen fertig in
// tests/fixtures/csharp. Aufruf: dotnet run tools/gen-csharp-fixtures.cs -- <ziel> [--large]
using WowTs3.Protocol;
using WowTs3.Server;
using WowTs3.Server.Storage;
using WowTs3.Storage;

var target = Path.GetFullPath(args[0]);
var large = args.Contains("--large");
if (Directory.Exists(target)) Directory.Delete(target, true);

var t0 = new DateTimeOffset(2026, 10, 1, 12, 0, 0, TimeSpan.Zero);
const string Alice = "SHA256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU";
const string Bob = "SHA256:ypeBEsobvcr6wjGzmiPcTaeG7/gUfE5yuYB3ha/uSLs";

var paths = new ServerPaths(target);
using (var db = ServerDatabase.Open(paths))
{
    db.Settings.Save(new ServerSettings { Name = "Testserver", WelcomeMessage = "Hallo öäü 🎧", Password = "geheim", RequireApproval = true, DefaultRole = Role.Guest });
    db.Channels.Save([
        new StoredChannel(1, 0, 0, "Lobby", "Thema", null, true, 0, 0),
        new StoredChannel(2, 0, 1, "Geschützt", null, "pw", false, 10, 5),
        new StoredChannel(3, 2, 0, "Unterkanal", null, null, false, 0, 0),
    ]);
    db.Members.Save(new Member(Alice, Role.Owner, "Alice", t0.UtcDateTime));
    db.Members.Save(new Member(Bob, Role.Member, "Bob", t0.AddHours(1).UtcDateTime));
    db.Bans.Add(new BanEntry("SHA256:bann", "Troll", "Spam", DateTime.MaxValue, Alice));
    db.Approvals.Save(new Approval(Bob, "Bob", ApprovalState.Allowed, t0.UtcDateTime, t0.AddMinutes(5).UtcDateTime, Alice));
    db.Events.Append([new ServerEvent(t0.UtcDateTime, "join", "Alice kam"), new ServerEvent(t0.AddSeconds(1).UtcDateTime, "leave", "Bob ging")]);
    db.Archive.Append(t0, Alice, "Alice", "Serverchat: erste Zeile", null);
    db.Archive.Append(t0.AddSeconds(2), Bob, "Bob", "Zweite Zeile mit Emoji 🙂", null);
}

Directory.CreateDirectory(paths.ChannelArchives);
using (var ch = new ChannelArchive(Path.Combine(paths.ChannelArchives, "1.sqlar")))
{
    for (var i = 0; i < 50; i++)
        ch.Append(t0.AddSeconds(i), i % 2 == 0 ? Alice : Bob, i % 2 == 0 ? "Alice" : "Bob", $"Nachricht {i} äöüß", null);
    var png = Convert.FromBase64String("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==");
    var img = new ArchivedImage("img01", 1, 1, png.Length);
    ch.PutImage(img, new MemoryStream(png));
    ch.Append(t0.AddMinutes(5), Alice, "Alice", "Bild", "img01");

    var small = Enumerable.Range(0, 100_000).Select(i => (byte)(i * 31)).ToArray();
    ch.PutFileContent("file01", new MemoryStream(small), small.Length);
    ch.SaveFile(new ArchivedFile("file01", "/", "klein.bin", false, small.Length, Alice, "Alice", t0, 1));
    ch.SaveFile(new ArchivedFile("dir01", "/", "Ordner", true, 0, Alice, "Alice", t0, 1));
    if (large)
    {
        var size = SqlarArchive.ChunkSize * 2 + 12345;
        var big = new byte[size];
        for (var i = 0; i < size; i++) big[i] = (byte)(i * 7 + i / 4096);
        ch.PutFileContent("file02", new MemoryStream(big), size);
        ch.SaveFile(new ArchivedFile("file02", "/Ordner", "gross.bin", false, size, Bob, "Bob", t0, 1));
    }

    ch.SaveBoard(new ArchivedBoard("board01", "Tafel", Alice, t0));
    ch.ChangeBoard("board01", [("s1", """{"kind":"rect","x":1,"y":2}"""), ("s2", """{"kind":"text","text":"Hallo"}""")]);
}

Directory.CreateDirectory(paths.DmArchives);
using (var server = new ChannelArchive(Path.Combine(target, "tmp-server.sqlar")))
{
    var set = new ArchiveSet(paths, server);
    set.Dm(Alice, Bob, true)!.Append(t0, Alice, "Alice", "Privat", null);
    set.Dispose();
}
foreach (var f in Directory.GetFiles(target, "tmp-server.sqlar*")) File.Delete(f);

// Ein Archiv, das nicht sauber geschlossen wurde: Datei und -wal kopieren,
// waehrend die Verbindung noch offen ist (wie nach einem Absturz).
var crashDir = Path.Combine(target, "crash");
Directory.CreateDirectory(crashDir);
var live = new ChannelArchive(Path.Combine(target, "live.sqlar"));
live.Append(t0, Alice, "Alice", "nur im WAL", null);
foreach (var f in Directory.GetFiles(target, "live.sqlar*"))
    File.Copy(f, Path.Combine(crashDir, Path.GetFileName(f)));
live.Dispose();
File.Delete(Path.Combine(target, "live.sqlar"));

foreach (var f in Directory.GetFiles(target, "*", SearchOption.AllDirectories))
    Console.WriteLine($"{new FileInfo(f).Length,12} {Path.GetRelativePath(target, f)}");
