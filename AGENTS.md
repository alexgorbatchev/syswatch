---
created_on: 2026-10-14 16:39
last_modified: 2026-10-15 09:56
status: current
---

# Syswatch fork

This fork is Alex's personal resource monitor, evolving into an application
shaped around his workflow. Prioritize his explicit product decisions over
preserving upstream presentation or feature parity. Dense is the primary design
target: understand which projects and tools consume resources, including helpers.

## Commands

- Run Dense: `cargo run --locked -- --dense`.
- Test: `cargo test --locked`.
- Format: `cargo fmt --all -- --check`.
- Lint: `cargo clippy --locked -- -W clippy::all`.
- Build the local executable: `cargo build --release --locked`.
- Preview the built app: `./target/release/syswatch --dense`.

## Product intent

- Preserve Dense's panel layout and dotted graphs while using the shared
  project/application tree for its process panel.
- Keep repository → checkout/worktree → tool → helpers/PIDs as distinct rows.
  Never flatten a repository and process into a combined label or remove the
  project/folder hierarchy merely because it contains one PID.
- Group each PID by its own working directory. Within Git, group directories by
  checkout/worktree root beneath the repository; outside Git, use the known
  working directory. A helper in another project belongs to its own project.
- Combine local repositories without a network origin by repository directory
  name, preserving each checkout path as a separate child row. Keep remote
  repositories identified by host and repository.
- Use parent relationships to attribute helpers to their owning tool within
  these location boundaries. Keep the owning tool available in process details.
- Docker is an explicit exception: Docker → containers → PIDs takes precedence
  over directory/project grouping. Use verified container membership; never
  assume every `containerd` process belongs to Docker.
- Without a known location, combine matching applications across OS users and
  installed versions. Show owners and executable paths in details. Runtime names
  and versioned launchers should resolve to the actual tool where evidence
  supports it; do not infer ownership from arbitrary command-line substrings.
- Collapse application/helper groups containing one PID into a process row.
  Preserve repository, folder, and container navigation rows.
- Sort tools before system-only groups, then by descending 5% CPU buckets.
  Within each bucket, put all repositories first and sort them alphabetically,
  including local repositories and repositories hosted outside GitHub.
  Sort remaining rows by name, with `~` paths before other names.
  Preserve selection and expansion as CPU usage changes row positions.
- "System" means OS/session infrastructure such as systemd, sshd, ssh-agent,
  gpg-agent, and D-Bus. PostgreSQL, Redis, Docker, VMs, and development tools stay
  with tools regardless of root ownership or OS installation paths.
- Show whole-machine CPU percentages without `.0`, resident memory, and CPU
  history. Match the CPU view's warning/critical colors. Memory below 1 GB is
  gray, using the app's 1024-based units.
- Color Dense's first-column labels by each row's whole-machine CPU: gray
  below 10%, normal foreground (white on dark themes) from 10% to below 30%,
  and red from 30%. Group labels use aggregate CPU; PID labels use their own.
  Render container IDs and `[system]` badges in gray at every CPU level.
  Preserve contrast on selected rows.
- Keep process count and PID in separate columns, with count before PID. Leave
  a count of one blank, reserve space for full PIDs, and show PIDs only on leaves.
- Use the same repository icon, Nerd Font `e725`, for every repository.

## Implementation and verification

- Read applicable skills and inspect source, dependencies, and repository state
  before editing. Ground implementation and completion claims in tool evidence.
- Reuse `src/cpu.rs` and `src/cpu/` for grouping, sorting, ownership, and stable
  row identities. Keep Dense rendering in `src/ui/dense/`; avoid a separate
  grouping implementation that drifts from Full.
- Keep grouping snapshot-based so replay and timeline scrubbing agree with live
  views. Persist new attribution metadata in recordings, and version incompatible
  postcard schema changes explicitly.
- Collect process/container metadata read-only. Keep actual OS PIDs and parent
  PIDs intact; display grouping must not fabricate process ancestry.
- Scrub new externally sourced strings in `src/collect/sanitize.rs` before
  display, including fields introduced for grouping.
- For runtime behavior changes, update a test file and require 90% code coverage
  excluding `scripts/`. Use red/green tests, then temporarily disable the change
  and confirm the tests fail. Report checks and coverage actually measured.
- Test aggregation without double counting, navigation, stable selection,
  narrow-terminal columns, and replay where the change affects them. Use
  termctrl for interactive terminal verification.
- Use `rg` or codegraph for searches, `.tmp/` for temporary artifacts, and
  `.workspaces/` for worktrees. Never use heredocs.
- Preserve unrelated work and unowned files. Stop and resync on concurrent index
  changes or Git conflicts; never stage, reset, or commit another agent's work.
- Update `docs/REFERENCE.md` for user-visible behavior changes. If a CLI usage
  skill exists, update it alongside changes to commands, flags, defaults, output,
  environment variables, or side effects.
- Persist standing guidance here by updating the relevant rule in place. Keep
  task history, progress, handoffs, and temporary approvals out of this file.
- Never publish releases, tags, packages, or production deployments without
  explicit authorization. Before committing or pushing, inspect the destination
  and changes for private internal domains; never put them on GitHub.

See `docs/REFERENCE.md` for the maintained user-facing behavior reference.
