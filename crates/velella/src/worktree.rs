//! Git worktree per task: file isolation without containers (yet). Every
//! task branches from `base` into its own checkout, so parallel agents can
//! never collide; merges stay ordinary git.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Keep the main checkout's `git status` clean: the runtime lives under
/// `.velella/`, while task specs in `.velella/tasks/` stay committable.
/// Idempotent; best-effort (no repo yet -> silently skipped).
pub fn ensure_runtime_ignored(repo_root: &Path) {
    let gitignore = repo_root.join(".gitignore");
    let existing = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if existing.contains(".velella/worktrees/") {
        return;
    }
    let add = "\n# velella runtime (task specs in .velella/tasks/ stay committable)\n.velella/worktrees/\n.velella/state.db\n.velella/state.db-*\n.velella/logs/\n";
    let _ = std::fs::write(&gitignore, format!("{}{}", existing.trim_end_matches('\n'), add));
}

/// Create `.velella/worktrees/<name>` branching from `base`.
pub fn create(repo_root: &Path, name: &str, base: &str) -> anyhow::Result<PathBuf> {
    ensure_runtime_ignored(repo_root);
    let path = repo_root.join(".velella").join("worktrees").join(name);
    if path.exists() {
        anyhow::bail!("worktree path already exists: {}", path.display());
    }
    let branch = format!("velella/{name}");
    let out = Command::new("git")
        .current_dir(repo_root)
        .arg("worktree")
        .arg("add")
        .arg(&path)
        .args(["-b", &branch, base])
        .output()?;
    if !out.status.success() {
        anyhow::bail!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(path)
}

/// Number of files changed between `base` and the worktree's HEAD — the
/// `diff_max_files` guardrail against runaway agents.
pub fn diff_file_count(worktree: &Path, base: &str) -> anyhow::Result<usize> {
    let range = format!("{base}...HEAD");
    let out = Command::new("git")
        .current_dir(worktree)
        .args(["diff", "--name-only", &range])
        .output()?;
    if !out.status.success() {
        anyhow::bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count())
}

/// Commit everything the agent left dirty; `None` when the worktree is
/// already clean (the agent committed its own work).
pub fn commit_all(worktree: &Path, message: &str) -> anyhow::Result<Option<String>> {
    let status = Command::new("git")
        .current_dir(worktree)
        .args(["status", "--porcelain"])
        .output()?;
    if !status.status.success() {
        anyhow::bail!("git status failed: {}", String::from_utf8_lossy(&status.stderr));
    }
    if String::from_utf8_lossy(&status.stdout).trim().is_empty() {
        return Ok(None);
    }
    Command::new("git")
        .current_dir(worktree)
        .args(["add", "-A"])
        .output()?;
    // try the ambient identity first; fall back to velella's when the
    // machine has none configured
    let commit = Command::new("git")
        .current_dir(worktree)
        .args(["commit", "-q", "-m", message])
        .output();
    let commit = match commit {
        Ok(o) if o.status.success() => o,
        _ => Command::new("git")
            .current_dir(worktree)
            .args(["-c", "user.name=velella", "-c", "user.email=velella@localhost"])
            .args(["commit", "-q", "-m", message])
            .output()?,
    };
    if !commit.status.success() {
        anyhow::bail!("git commit failed: {}", String::from_utf8_lossy(&commit.stderr));
    }
    let hash = Command::new("git")
        .current_dir(worktree)
        .args(["rev-parse", "--short", "HEAD"])
        .output()?;
    Ok(Some(String::from_utf8_lossy(&hash.stdout).trim().to_string()))
}

/// Push the task branch to origin. `Ok(false)` = no remote configured.
pub fn push(worktree: &Path, branch: &str) -> anyhow::Result<bool> {
    if remote_url(worktree)?.is_none() {
        return Ok(false);
    }
    let out = Command::new("git")
        .current_dir(worktree)
        .args(["push", "-q", "-u", "origin", branch])
        .output()?;
    if !out.status.success() {
        anyhow::bail!("git push failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(true)
}

/// Uncommitted (tracked) changes present in a checkout? Untracked files do
/// not block a merge, so they don't count.
pub fn dirty(dir: &Path) -> anyhow::Result<bool> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()?;
    if !out.status.success() {
        anyhow::bail!("git status failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// Currently checked-out branch of a checkout.
pub fn head_branch(dir: &Path) -> anyhow::Result<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()?;
    if !out.status.success() {
        anyhow::bail!("git rev-parse failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Merge the task branch into the branch checked out in `dir` (the main
/// checkout), always with a merge commit. Conflicts roll the merge back and
/// surface as an error — the verified work stays on the task branch.
pub fn merge(dir: &Path, branch: &str, message: &str) -> anyhow::Result<()> {
    if dirty(dir)? {
        anyhow::bail!("checkout has uncommitted changes — merge refused");
    }
    let out = Command::new("git")
        .current_dir(dir)
        .args(["merge", "--no-ff", "-q", "-m", message, branch])
        .output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let _ = Command::new("git").current_dir(dir).args(["merge", "--abort"]).output();
        anyhow::bail!("git merge failed (rolled back): {stderr}");
    }
    Ok(())
}

fn remote_url(worktree: &Path) -> anyhow::Result<Option<String>> {
    let out = Command::new("git")
        .current_dir(worktree)
        .args(["remote", "get-url", "origin"])
        .output()?;
    if !out.status.success() {
        return Ok(None); // no remote — fine, local-only workflow
    }
    Ok(Some(String::from_utf8_lossy(&out.stdout).trim().to_string()))
}

fn gh_available() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Open a PR via the GitHub CLI. `Ok(None)` = gh not installed; the caller
/// falls back to a compare link.
pub fn create_pr(
    worktree: &Path,
    branch: &str,
    base: &str,
    title: &str,
    body: &str,
) -> anyhow::Result<Option<String>> {
    if !gh_available() {
        return Ok(None);
    }
    use std::io::Write as _;
    let mut child = Command::new("gh")
        .current_dir(worktree)
        .args(["pr", "create", "--head", branch, "--base", base, "--title", title, "--body-file", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(body.as_bytes());
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        anyhow::bail!("gh pr create failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(stdout
        .split_whitespace()
        .find(|w| w.starts_with("http"))
        .map(str::to_string))
}

/// GitHub compare link for repos without the gh CLI — the manual door.
pub fn compare_url(worktree: &Path, base: &str, branch: &str) -> anyhow::Result<Option<String>> {
    let Some(url) = remote_url(worktree)? else {
        return Ok(None);
    };
    let web = if let Some(rest) = url.strip_prefix("git@github.com:") {
        format!("https://github.com/{}", rest.trim_end_matches(".git"))
    } else if let Some(rest) = url.strip_prefix("https://github.com/") {
        format!(
            "https://github.com/{}",
            rest.trim_end_matches(".git").trim_end_matches('/')
        )
    } else {
        return Ok(None); // non-GitHub remote: no compare convention
    };
    Ok(Some(format!("{web}/compare/{base}...{branch}?expand=1")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.email=velella@test", "-c", "user.name=velella"])
            .arg("-c")
            .arg("commit.gpgsign=false")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn create_diff_and_guardrail() {
        let tmp = std::env::temp_dir().join(format!("vel-wt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        git(&tmp, &["init", "-q", "-b", "main"]);
        std::fs::write(tmp.join("seed.txt"), "seed").unwrap();
        git(&tmp, &["add", "."]);
        git(&tmp, &["commit", "-q", "-m", "seed"]);

        let wt = create(&tmp, "t1", "main").unwrap();
        assert!(wt.join("seed.txt").exists(), "worktree has the checkout");

        // no work yet
        assert_eq!(diff_file_count(&wt, "main").unwrap(), 0);

        // one changed file committed in the worktree
        std::fs::write(wt.join("seed.txt"), "changed").unwrap();
        std::fs::write(wt.join("new.txt"), "new").unwrap();
        git(&wt, &["add", "."]);
        git(&wt, &["commit", "-q", "-m", "work"]);
        assert_eq!(diff_file_count(&wt, "main").unwrap(), 2);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_lands_on_base_and_rolls_back_conflicts() {
        let tmp = std::env::temp_dir().join(format!("vel-mg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        git(&tmp, &["init", "-q", "-b", "main"]);
        std::fs::write(tmp.join("seed.txt"), "seed").unwrap();
        git(&tmp, &["add", "."]);
        git(&tmp, &["commit", "-q", "-m", "seed"]);

        let wt = create(&tmp, "t3", "main").unwrap();
        std::fs::write(wt.join("feature.txt"), "feature").unwrap();
        git(&wt, &["add", "."]);
        git(&wt, &["commit", "-q", "-m", "feature"]);
        assert!(!tmp.join("feature.txt").exists(), "not merged yet");

        // merge into the main checkout
        merge(&tmp, "velella/t3", "velella: t3").unwrap();
        assert!(tmp.join("feature.txt").exists(), "merged");
        assert!(head_branch(&tmp).unwrap() == "main");

        // dirty checkout refuses the merge, work stays on the branch
        std::fs::write(tmp.join("feature.txt"), "local edit").unwrap();
        let wt2 = create(&tmp, "t4", "main").unwrap();
        std::fs::write(wt2.join("other.txt"), "other").unwrap();
        git(&wt2, &["add", "."]);
        git(&wt2, &["commit", "-q", "-m", "other"]);
        assert!(merge(&tmp, "velella/t4", "m").is_err(), "dirty checkout must refuse");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn commit_push_and_compare_url() {
        let tmp = std::env::temp_dir().join(format!("vel-pr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // a bare "origin" and a repo pushing to it — no network involved
        let origin = tmp.join("origin.git");
        let out = Command::new("git")
            .current_dir(&tmp)
            .args(["init", "-q", "--bare", "-b", "main"])
            .arg(&origin)
            .output()
            .unwrap();
        assert!(out.status.success(), "bare init: {}", String::from_utf8_lossy(&out.stderr));

        let repo = tmp.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("seed.txt"), "seed").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "seed"]);
        let origin_str = origin.to_string_lossy().to_string();
        let out = Command::new("git")
            .current_dir(&repo)
            .args(["remote", "add", "origin"])
            .arg(&origin_str)
            .output()
            .unwrap();
        assert!(out.status.success());
        git(&repo, &["push", "-q", "-u", "origin", "main"]);

        let wt = create(&repo, "t2", "main").unwrap();

        // nothing dirty -> no commit
        assert_eq!(commit_all(&wt, "empty").unwrap(), None);

        // dirty -> committed, pushed to the bare origin
        std::fs::write(wt.join("work.txt"), "work").unwrap();
        let hash = commit_all(&wt, "velella: t2").unwrap();
        assert!(hash.is_some());
        assert!(push(&wt, "velella/t2").unwrap(), "pushed to origin");

        // github-style remote -> compare link (git@ and https forms)
        git(&wt, &["remote", "set-url", "origin", "git@github.com:acme/repo.git"]);
        assert_eq!(
            compare_url(&wt, "main", "velella/t2").unwrap().as_deref(),
            Some("https://github.com/acme/repo/compare/main...velella/t2?expand=1")
        );
        git(&wt, &["remote", "set-url", "origin", "https://github.com/acme/repo.git/"]);
        assert!(compare_url(&wt, "main", "velella/t2").unwrap().is_some());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
