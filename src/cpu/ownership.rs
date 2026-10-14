//! Resolve owning tools from one snapshot, without consulting live /proc during replay.
use std::collections::HashMap;

use super::{location, GroupMode};
use crate::collect::{ProcTick, ProcessScope, Snapshot};

pub(super) struct Ownership<'a> {
    procs: &'a [ProcTick],
    indices: HashMap<u32, usize>,
    parents: Vec<Option<usize>>,
    owners: Vec<usize>,
}

impl<'a> Ownership<'a> {
    pub fn new(snap: &'a Snapshot) -> Self {
        Self::in_mode(snap, GroupMode::Application)
    }

    pub fn in_mode(snap: &'a Snapshot, mode: GroupMode) -> Self {
        let procs = &snap.procs;
        let indices: HashMap<_, _> = procs.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
        let mut parents: Vec<_> = procs
            .iter()
            .enumerate()
            .map(|(i, child)| {
                let parent = *indices.get(&child.ppid)?;
                if parent == i || boundary(child) || boundary(&procs[parent]) {
                    return None;
                }
                if mode == GroupMode::Project && location::of(child) != location::of(&procs[parent])
                {
                    return None;
                }
                Some(parent)
            })
            .collect();
        remove_cycles(&mut parents);
        for (i, parent) in parents.iter_mut().enumerate() {
            // A PID may have been reused between process-table reads. Check
            // after rejecting cycles so a malformed cycle cannot partly survive.
            if parent.is_some_and(|parent| matches!((procs[parent].start_time, procs[i].start_time), (Some(p), Some(c)) if p > c)) {
                *parent = None;
            }
        }

        // Resolve in parent-first order, iteratively: deep process chains must
        // not consume the call stack. A shell can inherit an owner but cannot
        // become the owner of an independently launched tool.
        let mut resolved = vec![false; procs.len()];
        let mut candidates = vec![None; procs.len()];
        for i in 0..procs.len() {
            let mut path = Vec::new();
            let mut current = Some(i);
            while let Some(index) = current.filter(|&index| !resolved[index]) {
                path.push(index);
                current = parents[index];
            }
            for index in path.into_iter().rev() {
                candidates[index] = parents[index]
                    .and_then(|parent| candidates[parent])
                    .or_else(|| (!shell(&procs[index])).then_some(index));
                resolved[index] = true;
            }
        }
        let owners = candidates
            .into_iter()
            .enumerate()
            .map(|(i, owner)| owner.unwrap_or(i))
            .collect();
        Self {
            procs,
            indices,
            parents,
            owners,
        }
    }

    pub fn owner(&self, p: &ProcTick) -> &'a ProcTick {
        &self.procs[self.owners[self.indices[&p.pid]]]
    }

    pub fn branch(&self, p: &ProcTick) -> Vec<&'a ProcTick> {
        let mut index = self.indices[&p.pid];
        let owner = self.owners[index];
        let mut path = vec![&self.procs[index]];
        while index != owner {
            let Some(parent) = self.parents[index] else {
                break;
            };
            index = parent;
            path.push(&self.procs[index]);
        }
        path.reverse();
        path
    }
}

fn executable_name(p: &ProcTick) -> &str {
    p.exe
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.rsplit(['/', '\\']).next())
        .unwrap_or(&p.name)
        .trim_end_matches(" (deleted)")
}

fn shell(p: &ProcTick) -> bool {
    matches!(
        executable_name(p),
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "dash"
            | "ash"
            | "ksh"
            | "tcsh"
            | "csh"
            | "nu"
            | "pwsh"
            | "powershell.exe"
            | "pwsh.exe"
            | "cmd.exe"
    )
}

fn boundary(p: &ProcTick) -> bool {
    p.scope == ProcessScope::System
        || prompt_shell(p)
        || matches!(
            executable_name(p),
            "" | "init"
                | "launchd"
                | "services.exe"
                | "svchost.exe"
                | "terminal"
                | "Terminal"
                | "iTerm2"
                | "alacritty"
                | "kitty"
                | "wezterm"
                | "wezterm-gui"
                | "ghostty"
                | "gnome-terminal-server"
                | "konsole"
                | "xterm"
                | "foot"
                | "footserver"
                | "tmux"
                | "screen"
                | "gnome-shell"
                | "plasmashell"
                | "sway"
                | "Hyprland"
                | "login"
                | "sshd"
                | "systemd"
                | "explorer.exe"
                | "WindowsTerminal.exe"
        )
}

fn prompt_shell(p: &ProcTick) -> bool {
    if !shell(p) {
        return false;
    }
    // A bare shell is a session boundary even when its launcher is an
    // unfamiliar terminal manager. Command/script shells retain ancestry.
    // Compare the complete invocation: splitting the flattened cmdline
    // would mistake text inside a command or a path for shell options.
    let cmd = p.cmd.trim();
    cmd.is_empty()
        || cmd == p.name
        || p.exe.as_deref() == Some(cmd)
        || cmd.strip_prefix('-') == Some(executable_name(p))
}

fn remove_cycles(parents: &mut [Option<usize>]) {
    let mut colors = vec![0u8; parents.len()];
    for start in 0..parents.len() {
        if colors[start] != 0 {
            continue;
        }
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current.filter(|&index| colors[index] == 0) {
            colors[index] = 1;
            path.push(index);
            current = parents[index];
        }
        if let Some(index) = current.filter(|&index| colors[index] == 1) {
            // Discard every edge in the cycle, independent of snapshot order.
            if let Some(first) = path.iter().position(|&i| i == index) {
                for &i in &path[first..] {
                    parents[i] = None;
                }
            }
        }
        for index in path {
            colors[index] = 2;
        }
    }
}
