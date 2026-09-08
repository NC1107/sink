# App identity

Status: implemented in the app-identity PR (ladder, client tracking, migration, mirror removal). The Rules UI is a follow-up.

## Problem

Sink keys an app by the first plausible stream property (`application.name`, then binary, then `media.name`, then `node.name`) but *displays* it by resolving a pid to a desktop entry.
The two disagree whenever a process names its streams something other than itself, so one program becomes several rows and several rules, and rules end up keyed on engines instead of apps.

## Evidence (verified on Nick's machine, 2026-09-08)

Config:

- 18 assignments; 9 belong to one game plus one game-across-updates: `Slay the Spire 2`, `FMOD Audio`, `SDL Application`, `java`, `LINK`, `Factorio: Space Age 2.1.8/2.1.9/2.1.11`, `Factorio 2.0.77`.
- `FMOD Audio -> Game 2` and `SDL Application -> Game 2` route every FMOD or SDL game. Silent misrouting.
- History says "Steam" for Rocket League and "WezTerm" for Firefox: the pid lookup trusts the cgroup, and a process started from a launcher or terminal inherits the launcher's scope.
- 9 rules have no history row: they route but cannot be seen or removed.

Live streams:

- Steam game (cs2, native client): `application.process.id` = `pipewire.sec.pid` = the real pid. Environment carries `SteamAppId=730`; `appmanifest_730.acf` names it "Counter-Strike 2". Slay the Spire 2 = 2868840, Factorio = 427520. Its cgroup is `app-steam@<uuid>.service`, the launcher's scope.
- The same game had a second stream with **no** pid or binary on the node; both were on its **Client** object. Sink does not track clients. `pactl` never shows a pid for that stream.
- Flatpak (Spotify sandbox, pulse path): the stream reports **pid 2**, which on the host is `kthreadd`. The client carries `pipewire.access = flatpak` and `pipewire.access.portal.app_id = com.spotify.Client`; `pipewire.sec.pid` is pipewire-pulse itself. Today's icon resolver reads `/proc/2` for every Flatpak app.
- Non-sandboxed pulse client: `application.process.id` is the real pid; `pipewire.sec.pid` is pipewire-pulse.

Precedent: Discord's detectable-games list (24,248 titles) keys a game by **executable name** and links a **Steam app id** (18,691 titles) - the same two signals as rows 2 and 5 below. Only 8 titles have native Linux executables, so on Linux the Steam id must lead: it identifies a Proton title whose exe is just `wine`.

Not games-only: version-in-name churn (Factorio; VLC embeds the LibVLC version), Electron apps collapsing when name and binary are both generic, `media.name` (a track title) becoming the identity when nothing better exists.

## Design

### 0. Track Client objects

The loop keeps `clients: HashMap<u32, props>` (insert on `global`, remove on `global_remove`). A stream's facts are its node props unioned with its client's props. Prerequisite for pid trust, Flatpak identity, and the pid-less-node case.

### 1. Which pid to trust

```
if client has pipewire.access.portal.app_id, or pipewire.access == "flatpak"
    -> no pid (sandbox namespace); identity comes from the app id
elif native client (pipewire.sec.pid == application.process.id)
    -> pid = sec.pid (kernel-verified)
elif application.process.id is set
    -> pid = that, but only if application.process.binary is absent
       or basename(/proc/pid/exe) == binary or exe is a known wrapper
    -> else no pid (some other namespace)
```

A pid is never cached on its own: process facts are cached per stream serial and evicted on that node's `global_remove`, so a recycled pid cannot inherit a dead stream's identity.

### 2. Wrapper values are never an identity

`WRAPPER_NAMES` (python3, java, node, mono, dotnet, electron, wine, wine64, `*-preloader`, `AppRun`, `sh`, `env`, `bwrap`) moves into the ladder: any row whose value is a wrapper is skipped, at every row, not just for Wine. This is what stops every Python app becoming "python3" and every AppImage becoming "AppRun". Row 5 is also skipped when `application.process.binary` ends in `.exe`, name-agnostic, so it survives whatever Wine calls its loader next.

### 3. Identity ladder

Canonical identity is `(kind, value)`; the first row that yields a non-wrapper value wins.

| # | kind | source | why it is stable |
|---|---|---|---|
| 1 | `flatpak` | client `pipewire.access.portal.app_id` | exact, sandbox-safe, no `/proc` |
| 2 | `steam` | `SteamAppId` from `/proc/<pid>/environ` | one id per game, native and Proton, survives updates |
| 3 | `appimage` | `APPIMAGE` from `/proc/<pid>/environ`, basename | the runtime sets it; exe is a FUSE mount otherwise |
| 4 | `desktop` | cgroup app scope (`app-*.scope/.service`, `snap.<pkg>.<app>`) or `GIO_LAUNCHED_DESKTOP_FILE`, **only if the entry's real Exec basename equals the process exe basename**, where "real" skips leading `env VAR=..`, `sh -c`, `flatpak-spawn --host` | the exec guard stops "WezTerm for Firefox"; the prefix skip stops false rejects |
| 5 | `exe` | basename of `/proc/<pid>/exe` | versionless; `factorio` is `factorio` forever |
| 6 | `binary` | `application.process.binary` | for Wine this is the .exe name, the right answer |
| 7 | `name` | `application.name` unless generic | today's primary, demoted |
| 8 | `node` | `node.name` | last resort |

`media.name` leaves the ladder; it is a stream title. Generic list grows with `SDL Application`, `FMOD Audio`, `LINK`, `Wine`.
Collapsing a browser's per-tab streams into one identity is the intended outcome, not a gap.

### 4. Display name from the same facts

`flatpak` -> `<app_id>.desktop` Name; `steam` -> appmanifest `name` (two-regex extraction over every library in `libraryfolders.vdf`, fallback to exe); `appimage` -> the file's basename without extension; `desktop` -> entry Name; `exe`/`binary` -> prettified; `name` -> as today.
One resolution feeds both, so they cannot disagree. Retires the "Steam"/"WezTerm" mislabels and the `/proc/2` Flatpak lookup.

### 5. Pid-less streams

- Native: the client lookup (section 0) resolves the cs2 case outright; a pid-less node shares its sibling's client.
- `pactl` fallback: no client props. A pid-less stream adopts a pid-bearing stream's identity only when **exactly one** candidate in the snapshot agrees on every property the pid-less stream carries, **and** the shared `application.name` is not a wrapper or generic value. So "Chromium" (Spotify and Chrome both claim it) never adopts. Anything else falls to the name ladder, which is today's behaviour.

### 6. Rules: identity plus matchers, additive on disk

```
Assignment {
  match_prop, match_value,      // unchanged, top-level: the primary matcher
  sink_name,
  identity: Option<(kind, value)>,
  matchers: Vec<(prop, value)>, // observed raw props, never generic or wrapper values
  last_matched: Option<u64>,
  schema: u32,
}
```

`match_prop`/`match_value` stay at the top level and are always populated, so an **old binary reading a new file keeps every rule** (it just ignores the new fields). New fields carry `serde(default)`. This is the downgrade case the first draft missed: `Assignments::load()` falls back to empty on a parse failure and the next save would have persisted the wipe.

### 7. WirePlumber mirror: removed

The mirror renders rules into a WirePlumber conf that routes streams to `sink_game` etc. Those sinks **only exist while Sink is running** - Sink creates them at launch and destroys them on quit. So the mirror can only ever act in the window between sink creation and the enforcer's first pass, about 200ms, and it can only match literal node properties, which cannot express `steam:730` or `flatpak:com.spotify.Client`. It is a second, weaker matching layer whose only remaining effect is the over-matching this plan exists to remove.
Decision: drop it. On first run of the new version, delete `90-sink-routing.conf` if present.

### 8. Migration - nobody loses a channel, nobody gains a wrong one

On every stream sighting:

1. Resolve canonical identity. If a rule has it, route and stamp `last_matched`.
2. Else find legacy rules whose `(match_prop, match_value)` equals a raw property of this stream. If found, create the canonical rule with that sink, mark the legacy rule `migrated`.
   This happens for every app that matches, including through a generic rule, so today's routing is preserved exactly on upgrade day.
3. Legacy rules are **never deleted automatically**. Generic-valued ones stay in force and are badged in the UI as engine-wide ("matches any SDL game") with a one-click remove. Deleting on first adoption was the v1 hazard: game B launching first would take game A's channel and erase the rule A depended on.
4. History rows merge by canonical identity.

### 9. Rules UI

Apps screen gains a Rules section: every assignment with display name, channel, `last_matched`, and the engine-wide badge.
Stale rules (no match in 90 days) get a cleanup action with a count. A button, not a prompt; consent, not a timer.
Closes the "routes but invisible" hole.

### 10. Backends

Native: full ladder. `pactl`: rows 2-8 whenever the sink-input carries a trusted pid; no client props, so no Flatpak id (name ladder, as today); adoption per section 5. Documented limitation.

## Verification

- Unit: the ladder over a `StreamFacts` struct (node props, client props, injected `/proc` reader). One test per row, the three pid-trust branches, wrapper skip at each row, `.exe` skip, exec-prefix skip, AppImage, snap scope, generic fall-through, adoption (unambiguous only, never on a wrapper name), migration (adopt-many, never delete, badge). Mutation-check each.
- Command level (mock backend + `TempConfig`): legacy rule adopts on first sighting; a second app through the same generic rule also adopts; nothing deleted; old-schema file round-trips; history merges.
- Live, isolated PipeWire: two `pw-cat` streams from one process with different `application.name` values produce one row and one rule; a `paplay` from a Flatpak sandbox resolves to the app id and never touches `/proc`.
- Real hardware: Slay the Spire 2 and Factorio on Nick's machine - one row each, rules migrate, engine rules show the badge.

## Risks

- `/proc` reads per new stream: already done for icons; cached per serial, evicted on removal.
- Appmanifest parsing: two regexes with a fallback to exe.
- Migration is additive, idempotent, logged.
- Removing the mirror changes nothing observable while Sink runs; the conf file is deleted once.

## Out of scope

Aliases (key on identity, migrate the same way). Electron collapse beyond the wrapper skip.

## Decisions taken

1. WirePlumber mirror: **dropped** (section 7).
2. Stale-rule cleanup: **button in Rules**, no launch prompt.

## Phases

1. Client tracking, pid trust, `StreamFacts`, the ladder and display name behind the existing `resolve_identity` call site. Tests. Fixes new installs and the Flatpak mislabel on its own.
2. Rule schema (additive) + migration + mirror removal. Tests, then live E2E. Fixes existing users.
3. Rules UI with badges and cleanup. Ship.
