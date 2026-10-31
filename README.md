<p align="center">
  <h1 align="center">SysWatch</h1>
  <p align="center">
    <strong>System diagnostics in your terminal.</strong>
  </p>
  <p align="center">
    <a href="https://github.com/alexgorbatchev/syswatch/releases"><img src="https://img.shields.io/github/v/release/alexgorbatchev/syswatch" alt="Release"></a>
    <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-blue" alt="Platform">
    <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
  </p>
</p>

<p align="center">
  <img src="demo-dense.gif" alt="SysWatch Dense: CPU, memory, network, per-core activity, disk throughput and processes on one screen" width="900">
</p>

<p align="center">
  <em>Live system activity in <code>syswatch --dense</code>.</em>
</p>

Monitor CPU, memory, disks, processes, GPU, power and network activity on macOS and Linux. Scrub recent history, record sessions, and investigate spikes with heuristic anomaly cards. SysWatch is read-only and runs without sudo; available measurements depend on your platform and permissions.

## Install

Download a versioned archive for your platform from this fork's
[GitHub Releases](https://github.com/alexgorbatchev/syswatch/releases).
The release workflow packages macOS and Linux binaries (x86_64 and aarch64),
static Linux variants, and an armv5te static build for older NAS hardware.
Each archive contains `syswatch` and its MIT license; `SHA256SUMS` covers all archives.

1. Download your archive and `SHA256SUMS` from the same release.
2. Verify the archive with `sha256sum --ignore-missing -c SHA256SUMS` on Linux,
   or `shasum -a 256 <archive>` on macOS and compare it with `SHA256SUMS`.
   Stop if the checksum differs.
3. Extract the archive with `tar xzf <archive>`, then install the executable in
   a directory on your `PATH`, for example `install -m 755 syswatch ~/.local/bin/syswatch`
   after creating `~/.local/bin` and adding it to your `PATH`.

Package-manager distributions of upstream SysWatch do not contain this fork's changes.

## Run

```bash
syswatch                       # twelve tabs, default 1 Hz refresh
syswatch --lite                # one 80×24 screen
syswatch --dense               # all main subsystems, designed for 130×44
syswatch --tab procs           # start on Processes
```

`1`–`9`, `0`, `-` and `+` switch tabs. `V` cycles views, `?` shows help, `q` quits.
Use `←` / `→` to scrub recent history and `End` to return live.

[Every keybinding](docs/REFERENCE.md#keys) · [Full-view demo](demo.gif)

## The tabs

| Key | Tab | Shows |
|---|---|---|
| 1 | Overview | Activity across subsystems |
| 2 | CPU | Application/project CPU totals, waiting pressure, process details and core activity |
| 3 | Memory | Memory use, swap and pressure |
| 4 | Disks | Disk read/write activity |
| 5 | Filesystems | Capacity, inode use and mounts |
| 6 | Procs | Processes, resource use, sorting and filtering |
| 7 | GPU | Utilisation and memory, where available |
| 8 | Power | Battery and available power measurements |
| 9 | Services | systemd or launchd services |
| 0 | Net | Network traffic |
| - | Timeline | Session events and history scrubbing |
| + | Insights | Anomalies and suggested tabs to investigate |

## Views

| View | Size | For |
|---|---|---|
| Full | Tabbed | Exploring individual subsystems |
| Lite (`--lite`) | 80×24 | SSH sessions and small terminals |
| Dense (`--dense`) | Designed for 130×44 | CPU, memory, network, cores, disk and processes together |

In Dense, `1`–`6` zoom a panel and `Esc` restores the grid.
[View details](docs/REFERENCE.md#dense-view) · [Lite demo](demo-lite.gif)

## Record and report

Press `R` to record an interactive session or `S` to save a snapshot.

```bash
syswatch --record --keep 24h          # foreground recording without a TUI
syswatch --replay session.swr         # replay a recording
syswatch snapshot --json             # one live sample
syswatch why                         # sample for 30s, explain detected anomalies
syswatch diff before.swr after.swr   # compare recordings
```

Live scrubbing holds 120 samples (two minutes at the default refresh rate).
Record longer sessions to disk. Headless recording rotates files and prunes old
chunks according to `--keep`; it runs until stopped.
[Recording and report details](docs/REFERENCE.md#whats-distinctive).

## Docs

| | |
|---|---|
| [Reference](docs/REFERENCE.md) | Controls, views, recording and reports |
| [Platform support](docs/REFERENCE.md#scope) | Sensors, permissions, NVIDIA support and ZFS |
| [Architecture](docs/REFERENCE.md#architecture) | Collectors, history, rendering and refresh model |
| [Security](SECURITY.md) | Security policy |

## Related

[NetWatch](https://github.com/matthart1983/netwatch) covers network diagnostics;
[DiskWatch](https://github.com/matthart1983/diskwatch) covers disk activity.
They share the terminal layout and palette.

## Thanks

Community packagers maintain the Nix and Arch packages.
[Repology](https://repology.org/project/syswatch/versions) tracks package versions.
File packaging issues with the packagers and
[SysWatch bugs here](https://github.com/matthart1983/syswatch/issues).

## License

MIT
