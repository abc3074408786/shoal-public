use super::git;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_REFERENCE_BYTES: usize = 1024;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ResolvedBase {
    pub reference: String,
    pub oid: String,
    pub inferred: bool,
}

/// Resolve at creation time; callers must freeze this OID for the new worktree.
pub(super) fn resolve(
    root: &Path,
    explicit: Option<&str>,
    configured: Option<&str>,
) -> Result<ResolvedBase> {
    // Some("") is an invalid choice, not permission to infer a different base.
    if let Some(reference) = explicit {
        return resolve_reference(root, reference, false).context("invalid explicit worktree base");
    }
    if let Some(reference) = configured {
        return resolve_reference(root, reference, false)
            .context("invalid configured worktree base");
    }

    // Only consult the locally stored symbolic remote default; never fetch or
    // guess from the current HEAD. A stale/dangling remote default may fall back.
    if let Ok(target) = git(
        root,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    ) {
        if let Ok(base) = resolve_reference(root, target.trim(), true) {
            return Ok(base);
        }
    }

    // Check exact local branch names, not tags or similarly named remote refs.
    let main = git(
        root,
        &["show-ref", "--verify", "--quiet", "refs/heads/main"],
    )
    .is_ok();
    let master = git(
        root,
        &["show-ref", "--verify", "--quiet", "refs/heads/master"],
    )
    .is_ok();
    let reference = match (main, master) {
        (true, false) => "refs/heads/main",
        (false, true) => "refs/heads/master",
        (true, true) => anyhow::bail!(
            "ambiguous worktree base: both main and master exist; choose an explicit or configured base"
        ),
        (false, false) => anyhow::bail!(
            "no worktree base: no valid origin/HEAD or local main/master branch; choose an explicit or configured base"
        ),
    };
    resolve_reference(root, reference, true)
}

fn resolve_reference(root: &Path, reference: &str, inferred: bool) -> Result<ResolvedBase> {
    anyhow::ensure!(
        reference.len() <= MAX_REFERENCE_BYTES,
        "worktree base exceeds {MAX_REFERENCE_BYTES} bytes"
    );
    // Check before trimming so trailing newlines and other controls are rejected.
    anyhow::ensure!(
        !reference.chars().any(char::is_control),
        "worktree base contains control characters"
    );
    let reference = reference.trim();
    anyhow::ensure!(!reference.is_empty(), "worktree base must not be empty");
    anyhow::ensure!(
        !reference.starts_with('-'),
        "worktree base must not start with an option"
    );
    let commit = format!("{reference}^{{commit}}");
    let oid = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &commit],
    )
    .with_context(|| format!("worktree base {reference:?} does not resolve to a commit"))?;
    Ok(ResolvedBase {
        reference: reference.to_owned(),
        oid: oid.trim().to_owned(),
        inferred,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    struct Repository {
        root: PathBuf,
        initial: String,
    }

    impl Repository {
        fn new() -> Self {
            // Refuse inherited Git directory overrides before any fixture writes.
            for name in [
                "GIT_DIR",
                "GIT_COMMON_DIR",
                "GIT_WORK_TREE",
                "GIT_INDEX_FILE",
                "GIT_OBJECT_DIRECTORY",
                "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                "GIT_NAMESPACE",
            ] {
                assert!(
                    std::env::var_os(name).is_none(),
                    "unset {name} for fixtures"
                );
            }
            let root = std::env::temp_dir().join(format!(
                "velella creation base 测试 {}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir(&root).unwrap();
            let mut repo = Self {
                root,
                initial: String::new(),
            };
            repo.git(&["init", "--quiet", "--template=", "-b", "fixture"]);
            repo.initial = repo.commit("initial", None);
            repo.set_ref("refs/heads/fixture", &repo.initial);
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            git(&self.root, args).unwrap().trim().to_owned()
        }

        fn commit(&self, message: &str, parent: Option<&str>) -> String {
            // Plumbing creates fixture-only commits without hooks, signing
            // overrides, an index, or changes to the user's Git configuration.
            let tree = self.git(&["mktree"]);
            let mut args = vec![
                "-c",
                "user.name=Creation Base Fixture",
                "-c",
                "user.email=creation-base@example.invalid",
                "commit-tree",
                &tree,
                "-m",
                message,
            ];
            if let Some(parent) = parent {
                args.extend(["-p", parent]);
            }
            self.git(&args)
        }

        fn set_ref(&self, reference: &str, oid: &str) {
            self.git(&["update-ref", reference, oid]);
        }

        fn origin_head(&self, oid: &str) {
            self.set_ref("refs/remotes/origin/trunk", oid);
            self.git(&[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/trunk",
            ]);
        }

        fn assert_base(
            &self,
            explicit: Option<&str>,
            configured: Option<&str>,
            reference: &str,
            oid: &str,
            inferred: bool,
        ) -> ResolvedBase {
            let base = resolve(&self.root, explicit, configured).unwrap();
            assert_eq!(base.reference, reference);
            assert_eq!(base.oid, oid);
            assert_eq!(base.inferred, inferred);
            base
        }
    }

    impl Drop for Repository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn explicit_then_configured_then_origin_head_take_priority() {
        let repo = Repository::new();
        let configured = repo.commit("configured", None);
        let origin = repo.commit("origin", None);
        repo.set_ref("refs/heads/chosen", &repo.initial);
        repo.set_ref("refs/heads/configured", &configured);
        repo.set_ref("refs/heads/main", &repo.initial);
        repo.set_ref("refs/heads/master", &repo.initial);
        repo.origin_head(&origin);

        repo.assert_base(
            Some("chosen"),
            Some("configured"),
            "chosen",
            &repo.initial,
            false,
        );
        repo.assert_base(
            Some("chosen"),
            Some("--invalid"),
            "chosen",
            &repo.initial,
            false,
        );
        repo.assert_base(None, Some("configured"), "configured", &configured, false);
        repo.assert_base(None, None, "refs/remotes/origin/trunk", &origin, true);
    }

    #[test]
    fn invalid_explicit_or_configured_reference_never_falls_back() {
        let repo = Repository::new();
        repo.set_ref("refs/heads/main", &repo.initial);
        repo.origin_head(&repo.initial);
        for reference in ["missing-branch", "HEAD^{tree}"] {
            assert!(resolve(&repo.root, Some(reference), Some("main")).is_err());
            assert!(resolve(&repo.root, None, Some(reference)).is_err());
        }
    }

    #[test]
    fn inputs_are_trimmed_bounded_and_cannot_be_options_or_contain_controls() {
        let repo = Repository::new();
        repo.set_ref("refs/heads/main", &repo.initial);
        for reference in [
            "",
            "   ",
            "-main",
            " --help ",
            "HEAD\n",
            "\tHEAD",
            "HE\0AD",
            "HEAD\u{7f}",
            "HEAD\u{85}",
        ] {
            assert!(resolve(&repo.root, Some(reference), None).is_err());
            assert!(resolve(&repo.root, None, Some(reference)).is_err());
        }
        let oversized = format!("{}HEAD", " ".repeat(MAX_REFERENCE_BYTES));
        assert!(resolve(&repo.root, Some(&oversized), None).is_err());
        assert!(resolve(&repo.root, None, Some(&oversized)).is_err());
        repo.assert_base(Some("  main  "), None, "main", &repo.initial, false);
        repo.assert_base(None, Some("  main  "), "main", &repo.initial, false);
    }

    #[test]
    fn unique_local_main_or_master_is_inferred() {
        for reference in ["refs/heads/main", "refs/heads/master"] {
            let repo = Repository::new();
            repo.set_ref(reference, &repo.initial);
            repo.assert_base(None, None, reference, &repo.initial, true);
        }
    }

    #[test]
    fn main_and_master_are_ambiguous_even_at_the_same_commit() {
        let repo = Repository::new();
        repo.set_ref("refs/heads/main", &repo.initial);
        repo.set_ref("refs/heads/master", &repo.initial);
        let error = resolve(&repo.root, None, None).err().unwrap().to_string();
        assert!(error.contains("ambiguous"), "{error}");
    }

    #[test]
    fn no_candidates_rejects_instead_of_using_head_tags_or_remote_branches() {
        let repo = Repository::new();
        repo.set_ref("refs/tags/main", &repo.initial);
        repo.set_ref("refs/tags/master", &repo.initial);
        repo.set_ref("refs/heads/main-topic", &repo.initial);
        repo.set_ref("refs/remotes/origin/main", &repo.initial);
        assert!(resolve(&repo.root, None, None).is_err());
        repo.assert_base(Some("HEAD"), None, "HEAD", &repo.initial, false);
    }

    #[test]
    fn dangling_origin_head_falls_back_only_to_unique_local_branch() {
        let repo = Repository::new();
        repo.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/missing",
        ]);
        assert!(resolve(&repo.root, None, None).is_err());
        repo.set_ref("refs/heads/main", &repo.initial);
        repo.assert_base(None, None, "refs/heads/main", &repo.initial, true);
        repo.set_ref("refs/heads/master", &repo.initial);
        assert!(resolve(&repo.root, None, None).is_err());
    }

    #[test]
    fn origin_head_must_be_symbolic_and_point_to_a_commit() {
        let repo = Repository::new();
        repo.set_ref("refs/remotes/origin/HEAD", &repo.initial);
        assert!(resolve(&repo.root, None, None).is_err());
        let tree = repo.git(&["mktree"]);
        repo.origin_head(&tree);
        assert!(resolve(&repo.root, None, None).is_err());
        repo.set_ref("refs/heads/master", &repo.initial);
        repo.assert_base(None, None, "refs/heads/master", &repo.initial, true);
    }

    #[test]
    fn detached_head_is_only_used_when_chosen_explicitly() {
        let repo = Repository::new();
        let detached = repo.commit("detached", None);
        repo.git(&["update-ref", "--no-deref", "HEAD", &detached]);
        assert!(resolve(&repo.root, None, None).is_err());
        repo.assert_base(Some("HEAD"), None, "HEAD", &detached, false);
        repo.assert_base(None, Some("HEAD"), "HEAD", &detached, false);
        repo.set_ref("refs/heads/main", &repo.initial);
        repo.assert_base(None, None, "refs/heads/main", &repo.initial, true);
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), detached);
    }

    #[test]
    fn explicit_tag_and_oid_are_valid_commit_references() {
        let repo = Repository::new();
        repo.set_ref("refs/tags/release", &repo.initial);
        repo.assert_base(Some("release"), None, "release", &repo.initial, false);
        repo.assert_base(
            Some(&repo.initial),
            None,
            &repo.initial,
            &repo.initial,
            false,
        );
    }

    #[test]
    fn branch_movement_changes_new_resolution_not_the_frozen_base() {
        let repo = Repository::new();
        repo.set_ref("refs/heads/main", &repo.initial);
        let frozen = repo.assert_base(None, None, "refs/heads/main", &repo.initial, true);
        let serialized = serde_json::to_string(&frozen.clone()).unwrap();
        let moved = repo.commit("moved", Some(&repo.initial));
        repo.set_ref("refs/heads/main", &moved);
        repo.assert_base(None, None, "refs/heads/main", &moved, true);
        repo.assert_base(Some("main"), None, "main", &moved, false);
        repo.assert_base(None, Some("main"), "main", &moved, false);
        assert_eq!(frozen.oid, repo.initial);
        let restored: ResolvedBase = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.oid, repo.initial);
        assert_eq!(restored.reference, "refs/heads/main");
        assert!(restored.inferred);
    }

    #[test]
    fn resolution_is_read_only_in_native_paths_with_spaces_and_unicode() {
        let repo = Repository::new();
        repo.set_ref("refs/heads/main", &repo.initial);
        let head = fs::read(repo.root.join(".git/HEAD")).unwrap();
        let refs = repo.git(&["show-ref"]);
        let worktrees = repo.git(&["worktree", "list", "--porcelain"]);
        let status = repo.git(&["status", "--porcelain"]);
        repo.assert_base(None, None, "refs/heads/main", &repo.initial, true);
        assert!(resolve(&repo.root, None, Some("missing")).is_err());
        assert_eq!(fs::read(repo.root.join(".git/HEAD")).unwrap(), head);
        assert_eq!(repo.git(&["show-ref"]), refs);
        assert_eq!(repo.git(&["worktree", "list", "--porcelain"]), worktrees);
        assert_eq!(repo.git(&["status", "--porcelain"]), status);
        assert!(!repo.root.join(".git/FETCH_HEAD").exists());
    }
}
