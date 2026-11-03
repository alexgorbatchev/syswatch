//! Snapshot-only location identities for grouping and ancestry boundaries.
use super::{display_path, GroupKey};
use crate::collect::ProcTick;

#[derive(PartialEq, Eq)]
pub(super) enum Location<'a> {
    Checkout {
        host: &'a str,
        repo: &'a str,
        path: &'a str,
    },
    Directory(&'a str),
}

pub(super) fn of(process: &ProcTick) -> Option<Location<'_>> {
    if let Some(project) = &process.project {
        Some(Location::Checkout {
            host: &project.host,
            repo: &project.repo,
            path: &project.checkout,
        })
    } else {
        process
            .cwd
            .as_deref()
            .filter(|path| !path.is_empty())
            .map(Location::Directory)
    }
}

impl Location<'_> {
    pub fn group(&self) -> (GroupKey, String, char) {
        match self {
            Self::Checkout { host, repo, .. } => (
                GroupKey::Project {
                    host: (*host).into(),
                    repo: (*repo).into(),
                },
                (*repo).into(),
                '\u{e725}',
            ),
            Self::Directory(path) => (
                GroupKey::Directory {
                    path: (*path).into(),
                },
                display_path(path),
                '\u{f07b}',
            ),
        }
    }
}
