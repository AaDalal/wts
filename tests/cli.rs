//! cli.rs: end-to-end tests that run the built `wts` binary against real git
//! (and, when available, jj) repos in throwaway temp dirs.
//!
//! Each test gets its own unique scratch dir (pid + atomic counter) and cleans
//! up at the end. Every git/wts invocation is made hermetic by pinning
//! `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` to /dev/null and `HOME` to the scratch
//! dir, so the developer's real config can't leak in and tests stay parallel-safe.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{env, fs};

static N: AtomicUsize = AtomicUsize::new(0);

const WTS_BIN: &str = env!("CARGO_BIN_EXE_wts");

/// A fresh, unique temp directory for one test, plus an empty `home` subdir we
/// point HOME at so nothing reads the developer's real home.
fn scratch() -> PathBuf {
    let id = N.fetch_add(1, Ordering::Relaxed);
    let dir = env::temp_dir().join(format!("wts-cli-{}-{id}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("home")).unwrap();
    dir
}

/// The hermetic env shared by every git and wts invocation: no global/system
/// git config, a private HOME, and fixed author/committer identity.
fn hermetic_env(scratch_dir: &Path) -> Vec<(String, String)> {
    let home = scratch_dir.join("home");
    vec![
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ("GIT_CONFIG_SYSTEM".into(), "/dev/null".into()),
        ("HOME".into(), home.to_string_lossy().into_owned()),
        ("GIT_AUTHOR_NAME".into(), "wts test".into()),
        ("GIT_AUTHOR_EMAIL".into(), "wts@example.com".into()),
        ("GIT_COMMITTER_NAME".into(), "wts test".into()),
        ("GIT_COMMITTER_EMAIL".into(), "wts@example.com".into()),
    ]
}

/// Run `git` in `dir` with the hermetic env, returning trimmed stdout. Panics on
/// non-zero exit (helpers should always succeed).
fn git(dir: &Path, scratch_dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .envs(hermetic_env(scratch_dir))
        .output()
        .expect("failed to spawn git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Make a git repo with one commit (subject `Initial commit`) and `wts.action.default cd`.
/// Returns the repo root path.
fn mk_git_repo(scratch_dir: &Path) -> PathBuf {
    let repo = scratch_dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, scratch_dir, &["init", "-b", "main"]);
    git(&repo, scratch_dir, &["config", "user.email", "wts@example.com"]);
    git(&repo, scratch_dir, &["config", "user.name", "wts test"]);
    fs::write(repo.join("README.md"), "hello\n").unwrap();
    git(&repo, scratch_dir, &["add", "."]);
    git(&repo, scratch_dir, &["commit", "-m", "Initial commit"]);
    git(&repo, scratch_dir, &["config", "wts.action.default", "cd"]);
    repo
}

struct WtsRun {
    status: Option<i32>,
    success: bool,
    #[allow(dead_code)]
    stdout: String,
    stderr: String,
    /// Contents (trimmed) of the WTS_CD_FILE if wts wrote one.
    cd_target: Option<String>,
}

/// Run the wts binary in `repo` with `args` and any `extra_env`, under the
/// hermetic env. A unique WTS_CD_FILE is provided; its contents (if any) are read
/// back as `cd_target`.
fn run_wts(
    repo: &Path,
    scratch_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> WtsRun {
    let id = N.fetch_add(1, Ordering::Relaxed);
    let cd_file = scratch_dir.join(format!("cd-{id}"));
    let _ = fs::remove_file(&cd_file);

    let mut env: HashMap<String, String> = hermetic_env(scratch_dir).into_iter().collect();
    env.insert(
        "WTS_CD_FILE".into(),
        cd_file.to_string_lossy().into_owned(),
    );
    for (k, v) in extra_env {
        env.insert((*k).into(), (*v).into());
    }

    // Inherit the parent env (notably PATH, so wts can find git/jj) but override
    // the hermetic keys; JJ_CONFIG is pinned so wts's jj calls don't read the
    // developer's jj config.
    let out = Command::new(WTS_BIN)
        .args(args)
        .current_dir(repo)
        .env("JJ_CONFIG", "/dev/null")
        .envs(&env)
        .output()
        .expect("failed to spawn wts");

    let cd_target = fs::read_to_string(&cd_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    WtsRun {
        status: out.status.code(),
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        cd_target,
    }
}

/// `git worktree list --porcelain` from the main repo.
fn worktree_porcelain(repo: &Path, scratch_dir: &Path) -> String {
    git(repo, scratch_dir, &["worktree", "list", "--porcelain"])
}

/// Whether a worktree at `path` is registered (by checking its `worktree` line).
fn worktree_registered(repo: &Path, scratch_dir: &Path, path: &Path) -> bool {
    let want = format!("worktree {}", path.to_string_lossy());
    // git canonicalizes paths in its listing; compare canonicalized basenames+parent.
    let canon = fs::canonicalize(path).ok();
    worktree_porcelain(repo, scratch_dir).lines().any(|l| {
        if let Some(p) = l.strip_prefix("worktree ") {
            if l == want {
                return true;
            }
            if let Some(c) = &canon {
                return fs::canonicalize(p).ok().as_deref() == Some(c.as_path());
            }
        }
        false
    })
}

fn cleanup(scratch_dir: &Path) {
    let _ = fs::remove_dir_all(scratch_dir);
}

// ---------------------------------------------------------------------------
// Git backend tests
// ---------------------------------------------------------------------------

#[test]
fn git_create_with_name() {
    let s = scratch();
    let repo = mk_git_repo(&s);

    let r = run_wts(&repo, &s, &["-n", "feature-x", "-a", "cd"], &[]);
    assert!(r.success, "wts failed: {}", r.stderr);

    let dest = s.join("repo-wts").join("feature-x");
    assert!(dest.is_dir(), "workspace folder missing: {}", dest.display());
    assert!(
        worktree_registered(&repo, &s, &dest),
        "worktree not registered:\n{}",
        worktree_porcelain(&repo, &s)
    );

    // Branch feature-x exists.
    let branches = git(&repo, &s, &["branch", "--list", "feature-x"]);
    assert!(!branches.is_empty(), "branch feature-x missing");

    // WTS_CD_FILE points at the new workspace.
    let cd = r.cd_target.expect("no cd target written");
    assert_eq!(
        fs::canonicalize(&cd).unwrap(),
        fs::canonicalize(&dest).unwrap()
    );

    cleanup(&s);
}

#[test]
fn git_auto_naming() {
    let s = scratch();
    let repo = mk_git_repo(&s);

    let short = git(&repo, &s, &["rev-parse", "--short", "HEAD"]);
    // Subject is "Initial commit" -> sanitized "initial-commit".
    let expected = format!("{short}-initial-commit");
    let expected: String = expected.chars().take(32).collect();

    let r = run_wts(&repo, &s, &["-a", "cd"], &[]);
    assert!(r.success, "wts failed: {}", r.stderr);

    let dest = s.join("repo-wts").join(&expected);
    assert!(
        dest.is_dir(),
        "expected folder {} missing; container had:\n{:?}",
        dest.display(),
        fs::read_dir(s.join("repo-wts")).map(|it| it
            .flatten()
            .map(|e| e.file_name())
            .collect::<Vec<_>>())
    );

    cleanup(&s);
}

#[test]
fn git_copy_untracked() {
    let s = scratch();
    let repo = mk_git_repo(&s);
    git(&repo, &s, &["config", "--add", "wts.copy", ".env*"]);

    fs::write(repo.join(".env"), "SECRET=1\n").unwrap();

    let r = run_wts(&repo, &s, &["-n", "copy-test", "-a", "cd"], &[]);
    assert!(r.success, "wts failed: {}", r.stderr);

    let copied = s.join("repo-wts").join("copy-test").join(".env");
    assert!(copied.is_file(), "untracked .env was not copied in");
    assert_eq!(fs::read_to_string(&copied).unwrap(), "SECRET=1\n");

    cleanup(&s);
}

#[test]
fn git_rm_by_name() {
    let s = scratch();
    let repo = mk_git_repo(&s);

    let c = run_wts(&repo, &s, &["-n", "feature-x", "-a", "cd"], &[]);
    assert!(c.success, "create failed: {}", c.stderr);
    let dest = s.join("repo-wts").join("feature-x");
    assert!(dest.is_dir());

    let r = run_wts(&repo, &s, &["rm", "feature-x"], &[]);
    assert!(r.success, "rm failed: {}", r.stderr);

    assert!(!dest.exists(), "folder not deleted");
    assert!(
        !worktree_registered(&repo, &s, &dest),
        "worktree still registered:\n{}",
        worktree_porcelain(&repo, &s)
    );
    let branches = git(&repo, &s, &["branch", "--list", "feature-x"]);
    assert!(branches.is_empty(), "branch feature-x not deleted: {branches}");

    cleanup(&s);
}

#[test]
fn git_rm_no_arg_from_inside_worktree() {
    let s = scratch();
    let repo = mk_git_repo(&s);

    let c = run_wts(&repo, &s, &["-n", "feature-x", "-a", "cd"], &[]);
    assert!(c.success, "create failed: {}", c.stderr);
    let dest = s.join("repo-wts").join("feature-x");
    assert!(dest.is_dir());

    // Run `wts rm` with current_dir = the worktree.
    let r = run_wts(&dest, &s, &["rm"], &[]);
    assert!(r.success, "rm failed: {}", r.stderr);

    assert!(!dest.exists(), "worktree folder not removed");

    // cd target should be the MAIN repo.
    let cd = r.cd_target.expect("no cd-back target written");
    assert_eq!(
        fs::canonicalize(&cd).unwrap(),
        fs::canonicalize(&repo).unwrap()
    );

    cleanup(&s);
}

#[test]
fn git_rm_refuses_nonexistent() {
    let s = scratch();
    let repo = mk_git_repo(&s);

    let r = run_wts(&repo, &s, &["rm", "nope"], &[]);
    assert!(!r.success, "rm of nonexistent should fail");
    assert_ne!(r.status, Some(0));
    assert!(
        r.stderr.contains("no such workspace"),
        "expected 'no such workspace' in stderr, got:\n{}",
        r.stderr
    );

    cleanup(&s);
}

#[test]
fn git_rm_dot_from_subdir_does_not_wipe_container() {
    let s = scratch();
    let repo = mk_git_repo(&s);

    let a = run_wts(&repo, &s, &["-n", "alpha", "-a", "cd"], &[]);
    assert!(a.success, "create alpha failed: {}", a.stderr);
    let b = run_wts(&repo, &s, &["-n", "beta", "-a", "cd"], &[]);
    assert!(b.success, "create beta failed: {}", b.stderr);
    let container = s.join("repo-wts");
    let alpha = container.join("alpha");
    let beta = container.join("beta");
    assert!(alpha.is_dir() && beta.is_dir());

    // `wts rm .` from a *subdirectory* of a worktree must delete nothing: `.`
    // isn't the worktree root (a direct child of the container), so it resolves
    // to no target. This guards the historical footgun where `.` was joined onto
    // the container path and `remove_dir_all` then wiped every sibling worktree.
    let sub = alpha.join("src/deep");
    fs::create_dir_all(&sub).unwrap();
    let r = run_wts(&sub, &s, &["rm", "."], &[]);
    assert!(!r.success, "wts rm . from a subdir should fail");
    assert!(
        r.stderr.contains("no such workspace"),
        "expected 'no such workspace', got:\n{}",
        r.stderr
    );

    // Container and both worktrees survive untouched.
    assert!(alpha.is_dir(), "alpha was deleted by `wts rm .`");
    assert!(beta.is_dir(), "beta was deleted by `wts rm .`");

    cleanup(&s);
}

#[test]
fn git_rm_dot_from_worktree_root_removes_just_it() {
    let s = scratch();
    let repo = mk_git_repo(&s);

    run_wts(&repo, &s, &["-n", "alpha", "-a", "cd"], &[]);
    run_wts(&repo, &s, &["-n", "beta", "-a", "cd"], &[]);
    let container = s.join("repo-wts");
    let alpha = container.join("alpha");
    let beta = container.join("beta");

    // `wts rm .` run from the worktree's own root resolves to that worktree.
    let r = run_wts(&alpha, &s, &["rm", "."], &[]);
    assert!(r.success, "rm . from worktree root failed: {}", r.stderr);
    assert!(!alpha.exists(), "alpha not removed");
    assert!(beta.is_dir(), "beta should be untouched");
    assert!(
        !worktree_registered(&repo, &s, &alpha),
        "alpha still registered:\n{}",
        worktree_porcelain(&repo, &s)
    );

    cleanup(&s);
}

#[test]
fn git_rm_dotdot_refuses_container() {
    let s = scratch();
    let repo = mk_git_repo(&s);

    run_wts(&repo, &s, &["-n", "alpha", "-a", "cd"], &[]);
    let container = s.join("repo-wts");
    let alpha = container.join("alpha");

    // `..` from inside a worktree resolves to the container, which is not a
    // worktree — refuse it rather than delete the whole container.
    let r = run_wts(&alpha, &s, &["rm", ".."], &[]);
    assert!(!r.success, "wts rm .. should fail");
    assert!(alpha.is_dir(), "alpha was deleted");
    assert!(container.is_dir(), "container was deleted");

    cleanup(&s);
}

#[test]
fn git_knob_detached_when_create_branch_false() {
    let s = scratch();
    let repo = mk_git_repo(&s);
    git(&repo, &s, &["config", "wts.createBranch", "false"]);

    let r = run_wts(&repo, &s, &["-n", "feature-x", "-a", "cd"], &[]);
    assert!(r.success, "wts failed: {}", r.stderr);

    let dest = s.join("repo-wts").join("feature-x");
    assert!(dest.is_dir());

    // The worktree record for our dest should be detached, with no branch line.
    let porcelain = worktree_porcelain(&repo, &s);
    let canon = fs::canonicalize(&dest).unwrap();
    let mut in_record = false;
    let mut saw_detached = false;
    let mut saw_branch = false;
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            in_record = fs::canonicalize(p).ok().as_deref() == Some(canon.as_path());
            continue;
        }
        if in_record {
            if line == "detached" {
                saw_detached = true;
            }
            if line.starts_with("branch ") {
                saw_branch = true;
            }
        }
    }
    assert!(saw_detached, "worktree not detached:\n{porcelain}");
    assert!(!saw_branch, "detached worktree unexpectedly has a branch:\n{porcelain}");
    // No feature-x branch created.
    assert!(git(&repo, &s, &["branch", "--list", "feature-x"]).is_empty());

    cleanup(&s);
}

#[test]
fn git_knob_branch_prefix() {
    let s = scratch();
    let repo = mk_git_repo(&s);
    git(&repo, &s, &["config", "wts.branchPrefix", "wts/"]);

    let c = run_wts(&repo, &s, &["-n", "feature-x", "-a", "cd"], &[]);
    assert!(c.success, "create failed: {}", c.stderr);

    // Branch is wts/feature-x, not feature-x.
    assert!(
        !git(&repo, &s, &["branch", "--list", "wts/feature-x"]).is_empty(),
        "prefixed branch wts/feature-x missing"
    );
    assert!(git(&repo, &s, &["branch", "--list", "feature-x"]).is_empty());

    // rm still deletes the prefixed branch.
    let r = run_wts(&repo, &s, &["rm", "feature-x"], &[]);
    assert!(r.success, "rm failed: {}", r.stderr);
    assert!(
        git(&repo, &s, &["branch", "--list", "wts/feature-x"]).is_empty(),
        "prefixed branch not deleted on rm"
    );

    cleanup(&s);
}

#[test]
fn git_knob_container_suffix() {
    let s = scratch();
    let repo = mk_git_repo(&s);
    git(&repo, &s, &["config", "wts.containerSuffix", ".worktrees"]);

    let r = run_wts(&repo, &s, &["-n", "feature-x", "-a", "cd"], &[]);
    assert!(r.success, "wts failed: {}", r.stderr);

    let dest = s.join("repo.worktrees").join("feature-x");
    assert!(
        dest.is_dir(),
        "container dir repo.worktrees not used: {}",
        dest.display()
    );
    assert!(!s.join("repo-wts").exists(), "default -wts container should not exist");

    cleanup(&s);
}

#[test]
fn git_no_default_action_errors() {
    let s = scratch();
    let repo = s.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &s, &["init", "-b", "main"]);
    git(&repo, &s, &["config", "user.email", "wts@example.com"]);
    git(&repo, &s, &["config", "user.name", "wts test"]);
    fs::write(repo.join("README.md"), "hello\n").unwrap();
    git(&repo, &s, &["add", "."]);
    git(&repo, &s, &["commit", "-m", "Initial commit"]);
    // NOTE: no wts.action.default configured.

    let r = run_wts(&repo, &s, &[], &[]);
    assert!(!r.success, "bare wts with no default action should fail");
    assert!(
        r.stderr.contains("no default action configured"),
        "expected 'no default action configured' message, got:\n{}",
        r.stderr
    );

    cleanup(&s);
}

// ---------------------------------------------------------------------------
// jj backend tests (gated on jj being installed)
// ---------------------------------------------------------------------------

/// True if a usable `jj` is on PATH.
fn jj_available() -> bool {
    Command::new("jj")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `jj` in `dir` under the hermetic env; panics on failure.
fn jj(dir: &Path, scratch_dir: &Path, args: &[&str]) -> String {
    let out = Command::new("jj")
        .args(args)
        .current_dir(dir)
        .envs(hermetic_env(scratch_dir))
        .env("JJ_CONFIG", "/dev/null")
        .output()
        .expect("failed to spawn jj");
    assert!(
        out.status.success(),
        "jj {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Make a colocated-free jj repo (`jj git init`) with one described change and a
/// `cd` default action set in the repo-local jj config.
fn mk_jj_repo(scratch_dir: &Path) -> PathBuf {
    let repo = scratch_dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    jj(&repo, scratch_dir, &["git", "init"]);
    jj(&repo, scratch_dir, &["config", "set", "--repo", "user.email", "wts@example.com"]);
    jj(&repo, scratch_dir, &["config", "set", "--repo", "user.name", "wts test"]);
    fs::write(repo.join("README.md"), "hello\n").unwrap();
    jj(&repo, scratch_dir, &["describe", "-m", "Initial change"]);
    jj(&repo, scratch_dir, &["config", "set", "--repo", "wts.action.default", "cd"]);
    repo
}

#[test]
fn jj_detected_and_create() {
    if !jj_available() {
        eprintln!("skipping jj_detected_and_create: jj not installed");
        return;
    }
    let s = scratch();
    let repo = mk_jj_repo(&s);

    // Sanity: the .jj dir exists so detect_backend picks jj.
    assert!(repo.join(".jj").is_dir(), "expected .jj dir for jj detection");

    let r = run_wts(&repo, &s, &["-n", "feature-x", "-a", "cd"], &[]);
    assert!(r.success, "wts (jj) create failed: {}", r.stderr);

    let dest = s.join("repo-wts").join("feature-x");
    assert!(dest.is_dir(), "jj workspace folder missing: {}", dest.display());

    // jj knows the workspace.
    let list = jj(&repo, &s, &["workspace", "list"]);
    assert!(
        list.lines().any(|l| l.starts_with("feature-x")),
        "jj workspace list missing feature-x:\n{list}"
    );

    let cd = r.cd_target.expect("no cd target written");
    assert_eq!(
        fs::canonicalize(&cd).unwrap(),
        fs::canonicalize(&dest).unwrap()
    );

    cleanup(&s);
}

#[test]
fn jj_rm_by_name() {
    if !jj_available() {
        eprintln!("skipping jj_rm_by_name: jj not installed");
        return;
    }
    let s = scratch();
    let repo = mk_jj_repo(&s);

    let c = run_wts(&repo, &s, &["-n", "feature-x", "-a", "cd"], &[]);
    assert!(c.success, "jj create failed: {}", c.stderr);
    let dest = s.join("repo-wts").join("feature-x");
    assert!(dest.is_dir());

    let r = run_wts(&repo, &s, &["rm", "feature-x"], &[]);
    assert!(r.success, "jj rm failed: {}", r.stderr);

    assert!(!dest.exists(), "jj workspace folder not deleted");
    let list = jj(&repo, &s, &["workspace", "list"]);
    assert!(
        !list.lines().any(|l| l.starts_with("feature-x")),
        "jj still lists feature-x after rm:\n{list}"
    );

    cleanup(&s);
}
