//! Display categories: OS/session infrastructure versus tools worth inspecting.
//! A systemd unit, root ownership, or an OS installation path does not imply
//! infrastructure: databases, containers, VMs and runtimes remain tools.

use std::{ffi::OsStr, path::Path};

use super::model::ProcessScope;

// Recognize helpers rather than demoting every OS-packaged binary. Unrecognized
// names stay with tools. CPU usage cannot change the category of a busy helper.
const SYSTEM_HELPERS: &[&str] = &[
    "systemd",
    "systemd-journald",
    "systemd-logind",
    "systemd-networkd",
    "systemd-resolved",
    "systemd-timesyncd",
    "systemd-udevd",
    "sd-pam",
    "(sd-pam)",
    "sshd",
    "ssh-agent",
    "gpg-agent",
    "dbus-daemon",
    "dbus-broker",
    "dbus-broker-launch",
    "cron",
    "crond",
    "agetty",
    "ModemManager",
    "NetworkManager",
    "wpa_supplicant",
    "avahi-daemon",
    "polkitd",
    "rsyslogd",
    "multipathd",
    "udisksd",
    "fwupd",
    "unattended-upgrade",
    "unattended-upgrades",
    "at-spi-bus-launcher",
    "at-spi2-registryd",
];

pub fn classify(exe: Option<&Path>, name: &OsStr) -> ProcessScope {
    let exe_name = exe.and_then(Path::file_name).filter(|s| !s.is_empty());
    let Some(basename) = exe_name.unwrap_or(name).to_str().filter(|s| !s.is_empty()) else {
        return ProcessScope::Unknown;
    };
    let basename = basename.strip_suffix(" (deleted)").unwrap_or(basename);
    let helper = SYSTEM_HELPERS.iter().any(|known| {
        basename == *known
            // Linux comm truncates names to 15 bytes. Only accept truncated
            // matches when the executable basename could not be obtained.
            || (cfg!(target_os = "linux") && exe_name.is_none() && known.get(..15) == Some(basename))
    });
    if helper {
        ProcessScope::System
    } else {
        ProcessScope::Tool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_helpers_are_system_even_without_a_system_unit() {
        for name in ["systemd", "sshd", "ssh-agent", "gpg-agent", "dbus-daemon"] {
            for dir in ["/usr/bin", "/nix/store/example/bin", "/home/dev/.local/bin"] {
                let path = Path::new(dir).join(name);
                assert_eq!(
                    classify(Some(&path), OsStr::new(name)),
                    ProcessScope::System,
                    "{name}"
                );
            }
            assert_eq!(
                classify(None, OsStr::new(name)),
                ProcessScope::System,
                "{name}"
            );
        }
    }

    #[test]
    fn development_tools_are_not_system_just_because_the_os_installed_them() {
        for name in [
            "postgres",
            "redis-server",
            "docker",
            "dockerd",
            "containerd",
            "qemu-system-x86_64",
            "qemu-ga",
            "node",
            "bash",
            "git",
            "nix",
            "chromium",
        ] {
            let path = Path::new("/usr/bin").join(name);
            assert_eq!(
                classify(Some(&path), OsStr::new(name)),
                ProcessScope::Tool,
                "{name}"
            );
            assert_eq!(
                classify(None, OsStr::new(name)),
                ProcessScope::Tool,
                "{name}"
            );
        }
    }

    #[test]
    fn executable_identity_takes_precedence_over_process_titles() {
        assert_eq!(
            classify(Some(Path::new("/usr/bin/node")), OsStr::new("systemd")),
            ProcessScope::Tool
        );
        assert_eq!(
            classify(
                Some(Path::new("/usr/bin/systemd (deleted)")),
                OsStr::new("MainThread")
            ),
            ProcessScope::System
        );
        assert_eq!(
            classify(
                Some(Path::new("/usr/bin/systemd-custom-tool")),
                OsStr::new("systemd")
            ),
            ProcessScope::Tool
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn truncated_linux_process_names_only_match_when_executable_is_missing() {
        for (name, exe) in [
            ("systemd-journal", "systemd-journald"),
            ("systemd-network", "systemd-networkd"),
            ("systemd-resolve", "systemd-resolved"),
            ("systemd-timesyn", "systemd-timesyncd"),
            ("unattended-upgr", "unattended-upgrade"),
            ("at-spi-bus-laun", "at-spi-bus-launcher"),
        ] {
            assert_eq!(
                classify(None, OsStr::new(name)),
                ProcessScope::System,
                "{name}"
            );
            let path = Path::new("/usr/bin").join(exe);
            assert_eq!(
                classify(Some(&path), OsStr::new(name)),
                ProcessScope::System,
                "{exe}"
            );
            let path = Path::new("/work").join(name);
            assert_eq!(
                classify(Some(&path), OsStr::new(name)),
                ProcessScope::Tool,
                "{name}"
            );
        }
        assert_eq!(classify(None, OsStr::new("systemd-")), ProcessScope::Tool);
    }

    #[test]
    fn missing_identity_remains_unknown_and_unknown_programs_remain_visible() {
        assert_eq!(classify(None, OsStr::new("")), ProcessScope::Unknown);
        assert_eq!(
            classify(None, OsStr::new("my-background-service")),
            ProcessScope::Tool
        );
    }
}
