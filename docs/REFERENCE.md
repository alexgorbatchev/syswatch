# SysWatch reference

[Back to the README](../README.md). Detailed controls, views, recording and platform notes.

## Build from source

```bash
git clone https://github.com/matthart1983/syswatch.git
cd syswatch
cargo build --release
./target/release/syswatch
```

## Usage

```bash
syswatch --dense               # every subsystem on one screen
syswatch                       # the twelve-tab tour, default 1Hz tick
syswatch --lite                # the one-screen Lite view
syswatch --tick 500            # 2Hz
syswatch --tab procs           # boot straight into a tab
syswatch --replay session.swr  # scrub a recorded session
syswatch --record --keep 24h   # unattended recording, no TUI (see below)
```

### Keys

```text
1 2 3 4 5 6 7 8 9   →  Overview / CPU / Mem / Disks / FS / Procs / GPU / Power / Services
0 - +               →  Net / Timeline / Insights
Tab / Shift-Tab     →  Cycle tabs
↑ / ↓               →  Select row (CPU, Procs, Services)
s                   →  Cycle sort (Procs, Services)
/ or f              →  Filter the table (Procs, Memory, Services)
← / →               →  Scrub session backward / forward
Home / End          →  Oldest sample / live
p                   →  Pause
g                   →  Graph style (bars / dots)
t                   →  Cycle theme (incl. "terminal" — uses your terminal's own palette)
,                   →  Settings (tick, theme, graph fade)
S / R               →  Snapshot to disk / record session
V                   →  Cycle views: Full → Lite → Dense
L                   →  Jump straight to the Lite view
?                   →  Help
q / Ctrl-C          →  Quit
```

### CPU view

The CPU tab starts with an assessment of CPU activity and available waiting-pressure
measurements, followed by applications with system-only groups at the bottom.
Within each section, applications sort into descending 5% CPU bands
(10–under 15%, 5–under 10%, 0–under 5%, for example). Within each band, all
repository rows come first, sorted alphabetically, including local repositories
and repositories hosted outside GitHub. Remaining rows sort by name,
with home-directory labels (`~` and `~/…`) before other names.
The same ordering applies to directory, checkout, application and process rows when expanded. Chrome
and its helpers share an application row. `Enter` expands a row and `↑` / `↓`
selects processes to inspect their command, user and working directory.

`[system]` marks groups containing only recognized OS/session helpers, including
systemd (both system and user managers), sshd, ssh-agent, gpg-agent and dbus.
Classification uses executable basenames, falling back to OS process names when
paths are unavailable; Linux's truncated process names are recognized too.
Root ownership, running as a system service, installation paths and CPU usage do
not determine the category. PostgreSQL, Redis, Docker, VMs, runtimes, shells and
other tools stay in the first section. Unrecognized programs and processes with
missing identities stay there too. Details show system, tool and unknown counts.
Mixed groups remain combined in the first section. Expanded process rows have
a separate PID column. The Procs column is blank for a count of one.
Application and helper branches containing only one PID render directly as process
rows, with no duplicate application wrapper. Project, directory and checkout rows
remain separate expandable rows even when they contain a single PID. Expand the
project and checkout, or the non-Git directory, to reach its process row.

Application rows include the owning tool and its descendant helpers, counting each
PID once in the top-level totals. Expand a tool to see its own PIDs and expandable
helper application groups; consecutive generations of the same application share
one helper group. Helper totals include their descendants. Process details show the
actual parent PID and owning tool. Independent Node workloads stay separate from
Node helpers owned by Pi or another tool.

Shells launched without arguments (including login-style names) stop ownership
traversal, separating tools from terminal/session launchers. Shells with unavailable
command lines also stop traversal; command/script shells pass ownership through.
Recognized OS/session helpers,
terminal emulators, session managers and multiplexers stop ownership traversal.
Missing parents, parent PIDs newer than their children, and cyclic parent links
are not used to infer ownership. Reparented/orphaned helpers therefore may appear
as independent applications. Attribution uses only the displayed snapshot, including
while scrubbing history or replaying a recording.

Press `b` to group by project or working directory instead. Attribution follows each process's own
working directory to its repository and reads `origin` locally, without contacting
the remote. Network origins group by host and repository; checkout rows distinguish
worktrees and combine working subdirectories beneath their checkout root.
Repositories without a network origin combine by repository directory name,
with separate checkout rows for each path. Network and local identities remain
separate. A known
working directory without a discovered repository gets its own directory row.
Projects and directories combine OS users; details list their owners.

Expand a checkout or directory to see applications with combined CPU, memory and
process counts, then expand an application to see its PIDs and helper groups.
Parent relationships group tools with helpers within the same checkout or non-Git
directory. Helpers in another location move to that location's group; their details
still show the global owning tool and actual parent PID. Processes with unavailable
working directories and no project metadata fall back to application groups.
Expansion is independent for each checkout and directory. Discovery results are
cached for up to 30 seconds.

Matching application names share one row across OS users and installed versions.
Names come from executable basenames or macOS bundles, falling back to the OS-reported
process name when the path is unavailable. Details list distinct owners and executable
paths, including how many processes have unavailable paths. Processes without either
a name or an executable remain individual. A distinct OS-reported Node process title
(such as `pi`) identifies the tool; generic `node` and `MainThread` names retain the
Node identity. Command-line substrings do not determine identity. In Project view,
applications combine within each checkout or non-Git directory;
different locations retain their own application rows.
Claude Code's native `claude/versions/<version>` executables and helpers share a
`claude` application row; their versioned executable paths remain in details.

Known applications use Nerd Font icons; all repositories use the same Git icon. Configure
your terminal with a [Nerd Font](https://www.nerdfonts.com/) to display these glyphs.
Names remain visible alongside icons. Chrome tab titles are not collected; use
Chrome's Task Manager to identify a particular tab or extension.

Editors, runtimes, containers and databases also have recognizable icons.
System services use category icons for networking, audio, security, scheduling
and other roles; sharing an icon does not combine different services into one row.
The Recent column grows with terminal width to show more history.

A muted `(deleted)` suffix means Linux reports that the running executable was
unlinked from disk. The process can continue running; the marker does not mean
it has stopped. Selecting the row shows this explanation in the detail panel.

The Memory column shows resident memory, summed for grouped rows. CPU values use
the Overview colors: green below 30%, warning at 30%, and critical at 70%.
CPU percentages in this tab represent the whole machine: one fully occupied core
on an eight-core machine is 12.5%, displayed rounded to a whole percentage.
Sorting uses the unrounded values. Process totals refresh approximately every three
seconds, independently of aggregate CPU samples, so the two need not match in a
given frame. CPU waiting is Linux PSI `some`, averaged over ten seconds: time when
at least some tasks waited for CPU, not a percentage of overall slowdown. Missing
pressure data is shown as unavailable. High CPU usage alone does not establish a fault.

`PgUp` / `PgDn` scrolls selected-row details. `c` expands core activity and redirects
`PgUp` / `PgDn` to core pages until cores are collapsed. On compact terminals,
expanded cores temporarily occupy the process-detail space. `←` / `→` scrubs the
assessment, groups and trends together. Open Settings with `,` and press `S` to
persist the grouping preference.

Recordings use format v6 to preserve executable paths, working directories, Git
attribution, system/tool categories and Docker container membership. This version
rejects older recording formats; older binaries cannot read v6 recordings.

### Dense view

`syswatch --dense`, or `V` to cycle Full → Lite → Dense. Every subsystem on one
130×44 screen — [the Dense demo](../demo-dense.gif). Where [Lite](#lite-view) is the
smallest useful thing, Dense is the largest: six boxes tiling the terminal with
**zero chrome rows** — no header, no tab bar, no status bar. Identity, uptime,
aggregate, sort state, page range and every keybind live inside the box borders,
so a heading costs no row.

```text
rows  0-11  cpu            full-height braille graph · axis · vitals
rows 12-23  mem  │ net     composition + history │ mirrored down/up
rows 24-31  cores │ disk   per-core grid │ read/write sparklines
rows 32-43  procs          detail-in-place + project process tree
```

**The mirror means "two directions of one flow."** Only `net` earns one:
download grows up from a shared axis, upload grows down, so traffic symmetry
becomes a shape — a restore is a cliff above the line, a backup a cliff below
it. CPU and memory have no opposing direction, so each gets one honest
full-height graph rather than a manufactured partner. Temperature is a bounded
scalar, not a flow, so it sits on the vitals row with a green→red meter.

Colour carries magnitude rather than identity: every cell is coloured by its own
height in the plot, so you see a spike's severity before you read the axis. That
split is deliberate — throughput graphs ramp cool→bright because a saturated disk
during a backup is *working*, and only bounded values where high genuinely is bad
(temperature, memory pressure, disk saturation) get the green→amber→red
vocabulary. Ramps are built from your theme, never hardcoded; on the 16-colour
`terminal` theme they step through the palette you already have rather than
synthesising colours you never chose.

`1`–`6` zoom a box to the whole frame — the process table at forty rows, or just
the network mirror — and `esc` restores the grid. Below 100×37 it falls back to a
three-box compact arrangement rather than cramming.

The process panel uses the CPU tab's project tree: repository → checkout/worktree
→ owning tool → helpers / PIDs. Known working directories outside Git get their
own folder row; processes without a known directory fall back to application
groups. `Enter` expands or collapses the selected group. `↑↓` / `j k`,
`Home` / `End`, and `PgUp` / `PgDn` navigate the visible tree. Selection and
expansion persist through zooming and switching views, independently of Full.
Both process trees show top-level labels without expansion markers. Expanded
children use `├─` and rounded `╰─` branches, with `│` connecting siblings.
PID leaves omit the horizontal stroke (`├` / `╰`) to sit one column shallower.
Folder icons switch to an open folder while expanded.
When a singleton becomes a group, selection follows its PID into the group;
when the selected process exits, selection moves to a nearby surviving row.

On Linux, Docker workloads override directory/project grouping and appear under
Docker → container ID → PIDs. Container rows stay expandable even with one PID.
Membership comes from host cgroups or Docker's `moby` containerd shims, without
querying a Docker socket. Container names are shown as short IDs; details retain
the full ID. Docker's daemon, CLI and proxy processes share the Docker root;
unrelated containerd workloads remain separate. Attribution depends on readable
host process metadata; containers inside a Docker Desktop VM are not exposed as
individual host PIDs by this collector.

Rows show aggregate process count, PID for individual processes, CPU as an
integer percentage of the whole machine, resident memory, and dotted CPU history
for top-level groups. Counts of one are blank. System-only groups sort last;
within each section, rows sort by descending 5% CPU buckets and then name.
Within each CPU bucket, all repositories come first, sorted by name; the
remaining rows follow, with `~` paths before other names. Container IDs and
`[system]` badges use dim gray text, lighter on the selected row for contrast.
First-column labels are gray below 10% CPU, normal foreground (white on dark
themes) from 10% to below 30%, and red at 30% and above. Group labels use their
aggregate CPU; individual PID labels use their own CPU. Container IDs and
`[system]` badges remain gray at every CPU level.
The numeric CPU column reaches the warning colour at 30% and the critical colour
at 70%, matching the CPU tab. Memory below 1 GB (1024³ bytes) uses neutral gray text, with a
lighter gray for selection contrast; larger values retain the memory color.
The two detail rows show owners and executable paths for groups,
or parent PID, user, working directory and command for individual processes.

Every number is measured over the window you can actually see, so a printed peak
is never one hiding in scrolled-off history. Graphs hold one sample per column
and fill from the right in real time, same as Lite: at the 1 Hz default a
120-column graph shows two minutes and takes two minutes to fill. The axis says
what it is actually showing.

That GIF was recorded with `vhs demo-dense.tape` at exactly 130×44 — the size
the grid was drawn at, so it shows the band heights the design intends rather than
whatever a taller or narrower terminal stretches them into — under three real
background loads: `yes` for the cores, rate-limited downloads for the net
mirror, and the file they land in for the disk trace. `--tick 250` for the same
reason as Lite: at 4 Hz the graphs fill inside a GIF instead of spending two
minutes empty, and nothing is fast-forwarded to get there. What the axes read
is the history they hold.

It is the sibling of [`netwatch`](https://github.com/matthart1983/netwatch)'s
Dense view — same primitives, same panel idiom, same `V` cycle — so muscle
memory carries between them the way it already does for Lite.

### Lite view

`syswatch --lite`, or `L` at any time. One screen at 80×24 answering one
question — *why is this machine hot, slow, or loud?* — with six keys and four
colors. It is not the full tool with tabs hidden; it is a different product for
someone with one machine, and the deliberate sibling of
[`netwatch --lite`](https://github.com/matthart1983/netwatch): identical grid
geometry, column positions, keys and palette, so muscle memory carries between
them.

<p align="center">
  <img src="../demo-lite.gif" alt="SysWatch Lite: one 80×24 screen with live CPU and memory charts, a vitals line carrying temp, fan, power and disk, and processes sorted by CPU — expanding one in place, then filtering the list live" width="820">
</p>

```text
q  quit     p  pause    /  filter (name or user)
↵  detail   L  full     ?  help          ↑↓ / j k move   Esc unwind
```

Recorded with `vhs demo-lite.tape` — braille area plots (`g`) over the faint
dot grid, with a right-bright / left-dim gradient — at
`--tick 250` so the charts fill inside a GIF. They hold one sample per column
and fill from the right in real time, so at the 1 Hz default the 78-column
chart takes 78 seconds. Nothing is fast-forwarded: the axis label measures the
history it is actually showing, and the sparkline header reports its own span,
so both say what the faster tick did.

CPU gets a three-row chart and memory two — when a machine feels wrong, CPU is
the answer more often than RAM. A single vitals line carries temp, fan, power
and disk throughput, each rendering `--` rather than moving when a sensor isn't
readable. Red appears only when the machine is actually in trouble — thermal
throttling, swap thrashing, critical memory pressure — on fixed thresholds with
hysteresis (three samples to fire, five to clear) so it never flaps. Memory
pressure comes from the kernel's own verdict where there is one (PSI on Linux,
`kern.memorystatus_vm_pressure_level` on macOS) rather than being inferred from
swap, so a Mac doing what Macs normally do doesn't read as an emergency. It follows
your theme and graph style like every other screen, and it is read-only, same
as the rest of syswatch.

## What's distinctive

**Insights tab.** Heuristic anomaly detection over the rolling session — swap thrash, runaway processes, disk full, memory pressure, high load, zombie parties — surfaced as plain-English cards with a suggested tab. The Overview's bottom strip and the tab bar's `[+]` badge keep them in sight from anywhere.

**Live scrubbing, full-session recording.** The Timeline tab's `←/→` rewinds every panel at once over the last 120 samples it keeps live — two minutes at the default 1 Hz tick, less at a faster one. For anything longer, `R` records the whole session to a `.swr` file as it runs; `--replay` scrubs that back afterward with no length limit, and `S` dumps the current snapshot to disk.

For recording without sitting at the terminal, `syswatch --record --keep 24h` runs headless (no TUI) and writes rotating hourly chunk files, pruning anything older than `--keep` (`30m` / `24h` / `7d` — any number plus s/m/h/d) as new ones are written. It's a plain foreground process — run it under `nohup`, a terminal multiplexer, or a user-level systemd/launchd unit if you want it to outlive your session; syswatch doesn't install anything or listen on the network on its own. `Ctrl-C` flushes the current chunk and exits cleanly. Recordings from `--record` land in a separate directory from `R`'s interactive ones, so an unattended run and a manual one you're keeping on purpose never collide or get pruned into each other.

**Honest about platform limits.** Where data needs sudo (`powermetrics` for fans, per-component power, GPU util on Apple Silicon) the tab shows what we *can* get for free and a one-line note about what's gated. Nothing is faked, nothing prompts.

**Non-interactive reports.** Four subcommands print a result and exit -- no TUI, no raw mode, safe to pipe into `jq` or drop into a script:

```bash
syswatch snapshot --json              # one live sample, full structure
syswatch insights --since 30s --json  # sample briefly, print whatever fired
syswatch why                          # same cards, as prose for a ticket
syswatch diff before.swr after.swr    # per-subsystem deltas + process changes
syswatch diff session.swr             # first vs last snapshot of one recording
```

`insights` and `why` sample the live host for `--since` (default 30s) before printing -- there's no background collector to ask for instant history, so both take as long as the window they're given. Longer catches slower heuristics (the memory-leak detector needs a couple of minutes of sustained growth to ever fire); the default is enough for the rest. `--json` on `snapshot`, `insights` and `diff` serializes the exact same `Snapshot` and `Insight` types the TUI itself renders, so a script sees what the tabs show, not a separate reporting-only shape.

## Anti-goals

- **Not multi-host.** For fleet view, use NetWatch's web dashboard.
- **No privileged daemon, no network exposure.** `--record --keep` can run for as long as you leave it, but it's a plain foreground process you start -- nothing gets installed, nothing listens on the network, no Prometheus endpoint. A recording is a file, not a database with a query API.
- **Not interactive remediation.** Read-only, deliberately. We don't kill, renice, unmount, or restart.
- **Not a logging product.** We surface OOM kills as a *signal* in Memory; we are not a log search UI.
- **Not pretty charts for screenshots.** Block sparklines, real numbers, no smooth curves, no themes-of-the-week.

## Scope

All twelve tabs render real data on macOS and Linux. Cross-platform collection via `sysinfo`; aggregate disk IO routes through [`netwatch-sdk`](https://github.com/matthart1983/netwatch-sdk) so SysWatch and the NetWatch agent share a single source of truth. Recording/Replay (`R` / `--replay`), unattended recording (`--record` / `--keep`), non-interactive reports (`snapshot` / `insights` / `diff` / `why`), Settings (`,`), Help (`?`), table filter (`/` or `f`, on Procs / Memory / Services), themes (`t`), the Lite view (`L` / `--lite`), the Dense view (`V` / `--dense`), and the graph-fade rendering are all live.

Lite's temp / fan / power vitals depend on platform sensors: Linux reads `/sys/class/hwmon`, `/sys/class/thermal` and RAPL; macOS needs IOKit/SMC access, so on Apple Silicon those three commonly render `--` while CPU, memory, disk and processes remain fully live.

**No sudo, ever.** GPU utilization, VRAM, and the renderer/tiler split on Apple Silicon come from `ioreg` (`AGXAccelerator PerformanceStatistics`); GPU temperature, per-rail power, and fans come from IOReport + SMC. Linux reads sysfs (`/sys/class/drm`, thermal zones, hwmon). Where a figure genuinely needs elevated access, the tab says so rather than prompting.

**Behind cargo features** — NVIDIA live GPU stats (`gpu-nvidia`, `nvml-wrapper`).

**ZFS.** On Linux hosts running ZFS, the ARC is counted as available memory rather than used. It is a filesystem cache that gives pages back under pressure, so leaving it in `used` reads as permanent memory pressure on a machine that has none.

## Architecture

```text
src/
├── main.rs              CLI + entry
├── app.rs               Event loop, tab state, scrub plumbing
├── collect/             One Collector per subsystem; Snapshot the wire format
│   ├── collector.rs     sysinfo-backed CPU/Mem/Procs/Net + dispatch
│   ├── gpu.rs           ioreg AGXAccelerator / sysfs DRM / nvml
│   ├── macos_sampler.rs Shared IOReport + SMC worker (GPU/power/fans)
│   ├── power.rs         ioreg / pmset / sysfs power_supply
│   ├── services.rs      launchctl / systemctl
│   └── ring.rs          Bounded history + nth_back for scrubbing
├── insights/            Pure functions over (History, &Snapshot)
├── tabs/                One file per tab; thin renderers over the model
└── ui/
    ├── chrome.rs        Header, tab bar, footer
    ├── palette.rs       Single source of color truth
    └── widgets.rs       block_bar, sparkline, panel
```

Refresh model: a 1 Hz fast loop reads CPU/Mem/Net/IO in-process every tick; the heavier collectors run on their own budgets — processes every ~3 s, Power/Services every 5 s, per-process bandwidth on a background thread — so the loop stays cheap regardless of tick rate. The UI redraws on tick or keypress.

