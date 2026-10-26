//! Read-only Linux Docker attribution. No Docker socket or daemon calls.
//! Host cgroup paths identify workloads even after shims are reparented to init.

use std::{ffi::OsString, path::Path};

pub(super) fn docker_container(pid: u32, exe: Option<&Path>, args: &[OsString]) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok();
        cgroup
            .as_deref()
            .and_then(from_cgroup)
            .or_else(|| from_shim(exe, args))
            .map(str::to_owned)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pid, exe, args);
        None
    }
}

#[cfg(any(target_os = "linux", test))]
fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(any(target_os = "linux", test))]
fn from_cgroup(text: &str) -> Option<&str> {
    for line in text.lines() {
        let Some(path) = line.splitn(3, ':').nth(2) else {
            continue;
        };
        let mut previous = "";
        for part in path.split('/') {
            let id = part
                .strip_prefix("docker-")
                .and_then(|p| p.strip_suffix(".scope"));
            if let Some(id) = id.filter(|id| valid_id(id)) {
                return Some(id);
            }
            if previous == "docker" && valid_id(part) {
                return Some(part);
            }
            previous = part;
        }
    }
    None
}

#[cfg(any(target_os = "linux", test))]
fn from_shim<'a>(exe: Option<&Path>, args: &'a [OsString]) -> Option<&'a str> {
    // Linux can deny /proc/PID/exe for a root-owned shim while exposing argv.
    // In that case use argv[0], still requiring its exact executable basename,
    // Docker's namespace, and a complete container ID. A known exe wins.
    let executable = exe.or_else(|| args.first().map(Path::new))?;
    let name = executable.file_name()?.to_str()?;
    let name = name.strip_suffix(" (deleted)").unwrap_or(name);
    if name != "containerd-shim" && !name.starts_with("containerd-shim-") {
        return None;
    }
    // Read OS argv, never search the flattened display command. containerd is
    // shared with other runtimes; only Docker's moby namespace qualifies.
    let mut namespace = None;
    let mut id = None;
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-namespace") => namespace = args.next().and_then(|arg| arg.to_str()),
            Some("-id") => id = args.next().and_then(|arg| arg.to_str()),
            _ => {}
        }
    }
    id.filter(|id| namespace == Some("moby") && valid_id(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_cgroups_accept_v1_v2_rootless_and_nested_paths() {
        let id = "a1".repeat(32);
        for path in [
            format!("/docker/{id}"),
            format!("/system.slice/docker-{id}.scope"),
            format!("/user.slice/user-1000.slice/user@1000.service/docker-{id}.scope/helpers"),
        ] {
            for prefix in ["0::", "7:cpu,cpuacct:", "4:memory:"] {
                let text = format!("5:pids:/unrelated\n{prefix}{path}\n");
                assert_eq!(from_cgroup(&text), Some(id.as_str()));
            }
        }
    }

    #[test]
    fn cgroups_reject_other_runtimes_partial_names_and_malformed_ids() {
        let id = "a".repeat(64);
        for text in [
            "".into(),
            format!("0::/kubepods/{id}"),
            format!("0::/not-docker/{id}"),
            format!("0::/system.slice/not-docker-{id}.scope"),
            "0::/docker/not-an-id".into(),
            format!("0::/docker/{}", "a".repeat(63)),
            format!("0::/docker/{}", "g".repeat(64)),
            format!("/docker/{id}"),
        ] {
            assert_eq!(from_cgroup(&text), None, "{text}");
        }
    }

    #[test]
    fn shim_attribution_requires_executable_namespace_and_complete_id_arguments() {
        let id = "b".repeat(64);
        let args: Vec<OsString> = [
            "shim",
            "-namespace",
            "moby",
            "-id",
            &id,
            "-address",
            "/run/containerd/containerd.sock",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let exe = Some(Path::new("/usr/bin/containerd-shim-runc-v2"));
        assert_eq!(from_shim(exe, &args), Some(id.as_str()));
        assert_eq!(from_shim(Some(Path::new("/bin/echo")), &args), None);
        assert_eq!(from_shim(None, &args), None);
        let mut unreadable_exe = args.clone();
        unreadable_exe[0] = "/usr/bin/containerd-shim-runc-v2".into();
        assert_eq!(from_shim(None, &unreadable_exe), Some(id.as_str()));
        assert_eq!(
            from_shim(Some(Path::new("/bin/echo")), &unreadable_exe),
            None
        );
        let mut unrelated = args.clone();
        unrelated[2] = "k8s.io".into();
        assert_eq!(from_shim(exe, &unrelated), None);
        for count in [0, 2, 4] {
            assert_eq!(from_shim(exe, &args[..count]), None);
        }
        let mut invalid = args;
        invalid[4] = "not-a-container".into();
        assert_eq!(from_shim(exe, &invalid), None);
    }
}
