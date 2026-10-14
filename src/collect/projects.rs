//! Local Git metadata, cached independently of process CPU refreshes.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::ProjectIdentity;

const CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct ProjectResolver {
    cache: HashMap<PathBuf, (Instant, Option<ProjectIdentity>)>,
}

impl ProjectResolver {
    pub fn prune(&mut self, now: Instant) {
        self.cache
            .retain(|_, (at, _)| now.duration_since(*at) < CACHE_TTL);
    }

    pub fn resolve(&mut self, cwd: &Path, now: Instant) -> Option<ProjectIdentity> {
        if let Some((at, value)) = self.cache.get(cwd) {
            if now.duration_since(*at) < CACHE_TTL {
                return value.clone();
            }
        }
        let value = discover(cwd);
        self.cache.insert(cwd.to_path_buf(), (now, value.clone()));
        value
    }
}

fn discover(cwd: &Path) -> Option<ProjectIdentity> {
    let cwd = cwd.canonicalize().ok()?;
    if !cwd.is_dir() {
        return None;
    }
    let repository = git2::Repository::discover(&cwd).ok()?;
    let checkout = repository.workdir()?.canonicalize().ok()?;
    let remote = repository.find_remote("origin").ok();
    let identity = remote
        .as_ref()
        .and_then(|r| r.url().ok())
        .and_then(remote_identity);
    let (host, repo) = identity.unwrap_or_else(|| {
        (
            String::new(),
            checkout
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        )
    });
    // Refuse lossy filesystem identities: two non-UTF8 paths must not merge.
    Some(ProjectIdentity {
        host,
        repo,
        checkout: checkout.to_str()?.into(),
    })
}

fn remote_identity(origin: &str) -> Option<(String, String)> {
    let url = gix_url::parse(origin.into()).ok()?;
    let mut host = url.host()?.to_ascii_lowercase();
    if let Some(port) = url.port {
        let standard = matches!(
            (&url.scheme, port),
            (gix_url::Scheme::Ssh, 22)
                | (gix_url::Scheme::Https, 443)
                | (gix_url::Scheme::Http, 80)
                | (gix_url::Scheme::Git, 9418)
        );
        if !standard {
            host = format!("{host}:{port}");
        }
    }
    let path = std::str::from_utf8(url.path.as_ref()).ok()?;
    let path = path.split(['?', '#']).next()?.trim_matches('/');
    let repo = path.strip_suffix(".git").unwrap_or(path);
    if repo.is_empty() {
        return None;
    }
    Some((host, repo.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> (tempfile::TempDir, git2::Repository) {
        std::fs::create_dir_all(".tmp").unwrap();
        let dir = tempfile::tempdir_in(".tmp").unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        (dir, repo)
    }

    #[test]
    fn discovers_origin_from_nested_cwd_without_retaining_credentials() {
        let (dir, repo) = repo();
        repo.remote("origin", "https://user:secret@github.com/acme/web.git")
            .unwrap();
        let nested = dir.path().join("src/deep");
        std::fs::create_dir_all(&nested).unwrap();
        let found = discover(&nested).unwrap();
        assert_eq!(found.host, "github.com");
        assert_eq!(found.repo, "acme/web");
        assert_eq!(
            std::path::Path::new(&found.checkout),
            dir.path().canonicalize().unwrap()
        );
        assert!(!serde_json::to_string(&found).unwrap().contains("secret"));
    }

    #[test]
    fn ssh_and_https_origins_have_the_same_identity() {
        assert_eq!(
            remote_identity("git@github.com:acme/web.git"),
            remote_identity("https://github.com/acme/web.git")
        );
        assert_eq!(
            remote_identity("ssh://git@git.example.com:2222/team/web.git"),
            Some(("git.example.com:2222".into(), "team/web".into()))
        );
        assert_eq!(remote_identity("../local.git"), None);
    }

    #[test]
    fn worktree_resolves_common_origin_and_its_own_checkout() {
        let (dir, repo) = repo();
        repo.remote("origin", "git@github.com:acme/web.git")
            .unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();
        let path = dir.path().join("feature");
        repo.worktree("feature", &path, None).unwrap();
        let found = discover(&path).unwrap();
        assert_eq!(found.repo, "acme/web");
        assert_eq!(
            std::path::Path::new(&found.checkout),
            path.canonicalize().unwrap()
        );
    }

    #[test]
    fn no_origin_is_local_and_missing_cwd_is_unattributed() {
        let (dir, _) = repo();
        let found = discover(dir.path()).unwrap();
        assert!(found.host.is_empty());
        assert!(discover(&dir.path().join("missing")).is_none());
    }

    #[test]
    fn cache_refreshes_changed_origin_at_expiry() {
        let (dir, repo) = repo();
        repo.remote("origin", "git@github.com:acme/web.git")
            .unwrap();
        let mut resolver = ProjectResolver::default();
        let now = Instant::now();
        assert_eq!(resolver.resolve(dir.path(), now).unwrap().repo, "acme/web");
        repo.remote_set_url("origin", "git@github.com:acme/api.git")
            .unwrap();
        assert_eq!(
            resolver.resolve(dir.path(), now + CACHE_TTL).unwrap().repo,
            "acme/api"
        );
    }
}
