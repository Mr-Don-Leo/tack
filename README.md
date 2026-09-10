# Tack

Trello-style boards, a fast personal to-do list, native desktop reminders and
lightweight automations — in one local-first app for Windows, macOS and Linux.

Adding a task takes one keystroke and one line of text. Everything else — boards,
columns, recurrence, rules — is there when a task grows into a project.

## What it does

**Boards.** One permanent **Main Board** that can never be deleted or archived,
plus as many boards as you want. Each board is a set of columns you define; cards
drag between and within them. A column can be marked as the board's "done"
column, which ties dropping a card there to completing the task.

**Tasks.** Title, description, due date and time, reminders, priority, labels,
checklist, notes, attachments, creation date and completion state. A task needs
only a title; everything else is optional.

**Reminders.** One or many per task, either anchored to the due date ("10 minutes
before") or pinned to a fixed instant, optionally repeating. They fire through
the OS notification system and keep working with every window closed.

**Recurring tasks.** Daily, weekdays, weekly (on chosen days), monthly, yearly,
with an interval, an end date or an occurrence count. Completing an occurrence
creates the next one, so completed history is preserved.

**Automations.** A trigger → conditions → actions rule engine, scoped globally or
to one board. Triggers include task created/completed/overdue, due date reached,
moved between columns, label added/removed, and a wall-clock schedule. Actions
include move, create, complete, change priority, add/remove label, change due
date, set a reminder, notify, duplicate and archive.

**Global views.** Today, Upcoming, Overdue, No due date, All tasks and Completed,
aggregated across every board, with board / label / priority filters.

**Search.** Across every board, over titles, descriptions, notes and checklist
items, with the matching text highlighted in context.

**Quick Add.** A global shortcut (`Ctrl/Cmd+Shift+Space` by default) opens a
capture bar over whatever you are doing. Type `Fix checkout invoice bug tomorrow
3pm !high #work` and it becomes a task on the Main Board with a due date, a
priority and a label.

**Tray / menu bar.** Add a task, see today's and upcoming tasks, open the app or
quit. Closing the window hides it instead of quitting, so reminders and
automations keep running.

## Running it

Prerequisites: [Rust](https://rustup.rs) and Node 20+.

```bash
npm install
npm run app          # development, with hot reload
npm run app:build    # produce an installable bundle for the current platform
```

### Linux build dependencies

```bash
# Fedora
sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel \
                 libayatana-appindicator-gtk3-devel librsvg2-devel

# Debian / Ubuntu
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
                 libayatana-appindicator3-dev librsvg2-dev patchelf rpm
```

### Installers

| Platform | Artifacts                          |
| -------- | ---------------------------------- |
| Linux    | `.rpm`, `.deb`, `.AppImage`        |
| macOS    | `.dmg` (arm64 and x64)             |
| Windows  | `.msi` and an NSIS `-setup.exe`    |

```bash
npm run bundle:linux     # rpm + AppImage
npm run bundle:windows   # run on Windows
npm run bundle:macos     # run on macOS
```

Each has to be built on its own operating system. Tauri cannot cross-compile:
a Windows bundle needs the MSVC toolchain and the WebView2 SDK, and a macOS
bundle needs Xcode's `codesign` and `hdiutil`. `.github/workflows/release.yml`
builds all four targets on their own runners and attaches the results to a
draft GitHub release — push a `v*` tag, or run the workflow manually.

Neither the macOS nor the Windows bundle is code-signed here. Without a
Developer ID certificate macOS shows a Gatekeeper warning on first launch, and
Windows shows a SmartScreen prompt. Add `APPLE_CERTIFICATE`,
`APPLE_SIGNING_IDENTITY` and the Windows signing secrets to the repository to
have `tauri-action` sign them.

An AppImage inherits the glibc of the machine that built it, so build it on the
oldest distribution you intend to support — the CI job uses Ubuntu 22.04 for
exactly this reason. `NO_STRIP=true` is set because linuxdeploy bundles a
binutils older than the `.relr.dyn` sections current distributions emit; the
libraries it copies in are already stripped by their own packaging, so nothing
is lost.

### Icons

`assets/logo.png` is the single source. After changing it:

```bash
python3 scripts/make-icon.py     # tray icons + the small sidebar copy
npx tauri icon assets/logo.png   # .icns, .ico and the PNG set
```

The tray is generated separately because the two platforms want opposite
things: macOS needs a monochrome *template* the OS tints to match the menu bar,
while Linux and Windows draw the icon over a panel whose colour is unknown, so
they get the full-colour logo.

## Where your data lives

Everything is local. Nothing is sent anywhere, and the app works with no network.

| What          | Where                                                     |
| ------------- | --------------------------------------------------------- |
| Database      | `<app data>/tack.db` (SQLite, WAL, `synchronous = FULL`)   |
| Backups       | `<app data>/backups/` — rolling, ten most recent kept      |
| Attachments   | `<app data>/attachments/<task id>/`                        |

`<app data>` is `~/.local/share/app.tack.desktop` on Linux,
`~/Library/Application Support/app.tack.desktop` on macOS, and
`%APPDATA%\app.tack.desktop` on Windows. Settings → Your data shows the exact
path and offers JSON export and import.

On startup the database is integrity-checked; if the primary file is unreadable
it is moved aside and the newest backup is restored in its place.

## Architecture

The parts that must survive the window being closed do not know a UI exists.

```
src-tauri/src/
  db.rs            SQLite setup, migrations, integrity check, backups
  models.rs        every type that crosses the IPC boundary
  store/           data access only — boards, lists, tasks, labels,
                   reminders, attachments, automations, settings, queries
  recurrence.rs    repeat maths, in local time, DST-aware
  automations.rs   trigger → condition → action engine
  ops.rs           product behaviour: the store write plus its consequences
  engine.rs        background thread: reminders, time triggers, backups
  notify.rs        native notifications
  nlp.rs           Quick Add natural-language parsing
  portability.rs   JSON export and import
  tray.rs          system tray / menu bar
  quickadd.rs      Quick Add window and global shortcut
  commands.rs      the IPC surface
src/
  api.ts           typed wrappers around the commands
  store.ts         route + last snapshot + subscriptions
  dom.ts           element helpers; no innerHTML anywhere
  components/      sidebar, board, task editor, list views, automations,
                   settings, and the custom control primitives
  styles/          design tokens, then everything built from them
```

The frontend is TypeScript with no framework: ~19 KB of gzipped JavaScript, a
system webview, and a Rust binary. There is no render loop and no polling from
the UI — the backend emits an event when data changes.

### Reminder action buttons

Snooze and Complete appear as buttons on the notification itself on Linux, where
the freedesktop notification spec delivers a button press back to the app. macOS
and Windows show the same notification without buttons, because neither path
available here routes one back. So every fired reminder also appears as an
in-app alert with Open / Snooze / Complete, which means both actions are always
one click away regardless of platform.

## Design

Apple HIG-inspired, driven entirely by CSS custom properties. Three skins —
`apple` (light and dark), `cyberpunk` (always dark) and `xp` (always light) —
each of which is a token override plus a handful of scoped flourishes.

No native `<select>`, `<input type="checkbox">` or `<input type="date">` is used
anywhere: WebKitGTK draws those itself and ignores the page's colours entirely,
which would put an unreadable light popup over a dark board. Dropdowns,
checkboxes, switches, segmented controls and the date picker are all built from
plain elements carrying the right ARIA roles.

## Security

- All content is rendered as DOM text nodes; the app has no `innerHTML` path.
- Attachments are copied into an app-managed store under a freshly generated
  name, so a filename from the filesystem cannot escape the directory. Opening
  one re-checks the path against the store first.
- User text is length-capped and stripped of control characters on the way in;
  every query is parameterised.
- A strict CSP; the asset protocol is scoped to the attachment directory.
- Single-instance, so two copies never contend for the same database.

Synchronisation is deliberately absent rather than half-built. The schema
already carries the timestamps a sync layer needs, and no core system assumes a
single device — but authentication, encryption, authorisation and conflict
resolution are their own design problem and are not pretended at here.

## Tests

```bash
cd src-tauri && cargo test    # recurrence, quick-add parsing, path safety
npm run build                 # typecheck + bundle
```
