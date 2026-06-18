//! wts — create or remove jj workspaces in a sibling `<repo>-wts/` folder.
//!
//! With no subcommand it creates a workspace; `wts rm <name>...` forgets
//! workspaces and deletes their folders. Human-facing messages go to stderr.
//!
//! A child process cannot change the parent shell's directory, so when wts wants
//! the shell to cd somewhere it writes the target path to the file named by the
//! `WTS_CD_FILE` env var (set by the `wts` shell function), which then cd's
//! there — into the new workspace on create, or back to the main repo after `rm`
//! deletes the folder you were standing in. Run without `WTS_CD_FILE` (e.g. a
//! direct `cargo run`) there's no shell to cd, so the path just appears on
//! stderr with the rest of the diagnostics.
//!
//! The default "cd into the new workspace" step can be replaced per-repo by a
//! `wts.action` jj config pointing at an executable script (see `run_action`).

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "wts",
    about = "Create (or remove) a jj workspace in a sibling <repo>-wts/ folder"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Parent revision the new workspace sits on (default: same parents as @)
    #[arg(short, long)]
    revision: Option<String>,

    /// Workspace name; if omitted, derived from the parent revision's
    /// description (sanitized: lowercase, dashes, <=32 chars)
    #[arg(short, long)]
    name: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Remove workspaces: jj workspace forget + delete each `<repo>-wts/<name>` folder
    Rm {
        /// Workspace name(s) to remove; omit to remove the current workspace
        /// (only valid when run from inside a `<repo>-wts/<name>` workspace)
        names: Vec<String>,
    },
}

fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("wts: error: {}", msg.as_ref());
    exit(1);
}

/// Tell the shell wrapper where to cd by writing the path to the scratch file it
/// names in `WTS_CD_FILE`. With no `WTS_CD_FILE` (e.g. a direct `cargo run`,
/// outside the wrapper) there's nothing to cd, so this is a no-op — the path is
/// already on stderr via the "creating workspace …" message.
fn emit_cd(path: &Path) {
    let Some(file) = env::var_os("WTS_CD_FILE") else { return };
    if let Err(e) = fs::write(&file, format!("{}\n", path.display())) {
        eprintln!("wts: could not record cd target: {e}");
    }
}

/// The directory under `dest` mirroring where you currently are within `source`,
/// so running `wts` from `repo/src/foo` lands you in `<new-ws>/src/foo`. Falls
/// back to `dest` itself when you're at the source root, the path can't be
/// resolved, or that subdirectory wasn't carried into the new workspace.
fn mirror_subpath(source: &Path, dest: &Path) -> PathBuf {
    match env::current_dir() {
        Ok(cwd) => mirror_subpath_from(source, dest, &cwd),
        Err(_) => dest.to_path_buf(),
    }
}

/// Core of [`mirror_subpath`], with `cwd` passed in so it's testable.
fn mirror_subpath_from(source: &Path, dest: &Path, cwd: &Path) -> PathBuf {
    let canon = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let Some(rel) = canon(cwd)
        .strip_prefix(canon(source))
        .ok()
        .map(Path::to_path_buf)
        .filter(|rel| !rel.as_os_str().is_empty())
    else {
        return dest.to_path_buf();
    };
    let target = dest.join(rel);
    if target.is_dir() {
        target
    } else {
        dest.to_path_buf()
    }
}

/// Path to the configured `wts.action` script, if any. A leading `~/` expands to
/// `$HOME`; an unset or empty value yields `None`.
fn action_script() -> Option<String> {
    match jj_capture(&["config", "get", "wts.action"]) {
        Ok(s) if !s.trim().is_empty() => Some(expand_tilde(s.trim())),
        _ => None,
    }
}

fn expand_tilde(s: &str) -> String {
    match s.strip_prefix("~/") {
        Some(rest) => match env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => s.to_string(),
        },
        None => s.to_string(),
    }
}

/// Run the configured `wts.action` in place of the default cd. The script
/// carries its own shebang (fish, bash, python, rust-script, …) and is run with
/// the new workspace as both its working directory and its sole argument, and
/// with that path also exported as `$WTS_DIR`. Stdio is inherited so interactive
/// actions (opening an editor, starting a shell) work. The action's exit code
/// becomes ours — the workspace was already created, so this only reports how
/// the action itself fared.
fn run_action(script: &str, dest: &Path) {
    eprintln!("wts: running wts.action ({script})");
    let status = Command::new(script)
        .arg(dest)
        .env("WTS_DIR", dest)
        .current_dir(dest)
        .status()
        .unwrap_or_else(|e| die(format!("failed to run wts.action '{script}': {e}")));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

/// Run a jj command and return trimmed stdout, or the trimmed stderr as Err.
fn jj_capture(args: &[&str]) -> Result<String, String> {
    let out = Command::new("jj")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run jj: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// lowercase, non-alphanumerics collapsed to single dashes, no leading/trailing
/// dash, capped at 32 chars.
fn sanitize(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        let lc = c.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() {
            out.push(lc);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.truncate(32);
    out.trim_matches('-').to_string()
}

/// Copy the files matched by the `wts.copy` jj config setting from the source
/// workspace into the freshly-created one. These are the untracked/ignored files
/// (e.g. `AGENTS.override.md`, `.env`) that jj does not carry into a new
/// workspace on its own. Unset config copies nothing; missing matches are
/// skipped silently — a copy failure is a warning, never fatal.
fn copy_configured_files(source: &Path, dest: &Path) {
    let patterns = copy_patterns();
    let base = glob::Pattern::escape(&source.to_string_lossy());
    let mut copied = 0usize;
    for pat in &patterns {
        let entries = match glob::glob(&format!("{base}/{pat}")) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("wts: ignoring bad wts.copy pattern '{pat}': {e}");
                continue;
            }
        };
        for path in entries.flatten() {
            let Ok(rel) = path.strip_prefix(source) else { continue };
            match copy_path(&path, &dest.join(rel)) {
                Ok(n) => copied += n,
                Err(e) => eprintln!("wts: failed to copy {}: {e}", rel.display()),
            }
        }
    }
    if copied > 0 {
        eprintln!("wts: copied {copied} file(s) into the new workspace");
    }
}

/// Glob patterns to copy, read from the `wts.copy` jj config table
/// (`wts.copy.<label> = "<glob>"`). A table is required because jj *merges*
/// tables across config layers, so a per-repo entry extends the user-level set
/// rather than replacing it. Entry keys are just labels; the string values are
/// the globs. Unset (or any non-table value) yields none. `jj config list`
/// prints valid TOML, so we parse it directly.
fn copy_patterns() -> Vec<String> {
    let listing = jj_capture(&["config", "list", "wts.copy"]).unwrap_or_default();
    if listing.is_empty() {
        return vec![];
    }
    let table: toml::Table = match listing.parse() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("wts: ignoring unparseable wts.copy config: {e}");
            return vec![];
        }
    };
    match table.get("wts").and_then(|w| w.get("copy")) {
        Some(toml::Value::Table(t)) => {
            // Sorted for a deterministic copy order regardless of layer/merge.
            let mut pats: Vec<String> =
                t.values().filter_map(|v| v.as_str().map(str::to_string)).collect();
            pats.sort();
            pats
        }
        _ => vec![],
    }
}

/// Recursively copy a file or directory, creating parent dirs as needed, and
/// return the number of files written.
fn copy_path(src: &Path, dst: &Path) -> std::io::Result<usize> {
    if fs::symlink_metadata(src)?.is_dir() {
        fs::create_dir_all(dst)?;
        let mut n = 0;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            n += copy_path(&entry.path(), &dst.join(entry.file_name()))?;
        }
        return Ok(n);
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dst)?;
    Ok(1)
}

/// The jj repo root of the current workspace.
fn workspace_root() -> PathBuf {
    PathBuf::from(
        jj_capture(&["workspace", "root"])
            .unwrap_or_else(|e| die(format!("not inside a jj repo: {e}"))),
    )
}

/// Resolve the `<repo>-wts/` container and the main repo path, whether `root` is
/// the main repo itself or one of its `<repo>-wts/<name>` workspaces.
fn resolve_layout(root: &Path) -> (PathBuf, PathBuf) {
    let parent = root
        .parent()
        .unwrap_or_else(|| die("repo root has no parent directory"));
    let base = root
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_else(|| die("cannot determine repo name from root path"));
    // If the parent dir is itself a `<repo>-wts` container, we're inside a
    // workspace: the container is the parent, the main repo its sibling.
    if let Some(pb) = parent.file_name().and_then(OsStr::to_str) {
        if let Some(repo_name) = pb.strip_suffix("-wts") {
            let main_repo = parent
                .parent()
                .map(|g| g.join(repo_name))
                .unwrap_or_else(|| die("cannot resolve main repo path"));
            return (parent.to_path_buf(), main_repo);
        }
    }
    (parent.join(format!("{base}-wts")), root.to_path_buf())
}

/// The jj name of the workspace we're currently in, if it's a `<repo>-wts/<name>`
/// worktree rather than the main repo. Returns None from the main repo (so a
/// no-name `rm` there can't target the default workspace).
///
/// jj — not the folder name — is the source of truth for the name: we ask which
/// workspace owns the current working copy. wts creates each workspace with its
/// folder name as the jj name, but the two can diverge (e.g. `jj workspace
/// rename`), and `jj workspace forget` needs the jj name. The folder name is
/// only a fallback for when jj can't give an unambiguous answer (e.g. two
/// workspaces sharing a working-copy commit).
fn current_workspace_name(root: &Path) -> Option<String> {
    // Gate on actually being inside a `-wts` container; this is what keeps the
    // main/default workspace off-limits to a no-name `rm`.
    let parent = root.parent()?;
    parent.file_name().and_then(OsStr::to_str)?.strip_suffix("-wts")?;

    let from_jj = jj_capture(&[
        "workspace",
        "list",
        "--ignore-working-copy",
        "-T",
        "if(target.current_working_copy(), name ++ \"\\n\", \"\")",
    ])
    .ok()
    .and_then(|out| {
        let mut names: Vec<String> = out
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        // Exactly one current working copy is the unambiguous case; anything
        // else (none, or several sharing a commit) falls back to the folder.
        (names.len() == 1).then(|| names.remove(0))
    });

    from_jj.or_else(|| root.file_name().and_then(OsStr::to_str).map(str::to_string))
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::Rm { names }) => do_rm(names),
        None => do_create(cli.revision, cli.name),
    }
}

fn do_create(revision: Option<String>, name: Option<String>) {
    let root = workspace_root();
    let (container, _main_repo) = resolve_layout(&root);

    // Workspace name: explicit --name, else derived from the parent revision.
    let workspace_name = match &name {
        Some(n) => sanitize(n),
        None => {
            // No --revision => the new working copy shares @'s parents, so the
            // base revision we name after is @-.
            let src_rev = revision.clone().unwrap_or_else(|| "@-".to_string());
            // Grab the short change id and the description's first line in one
            // shot (tab-separated) so we can prefix the name with the revision.
            let raw = jj_capture(&[
                "log", "--no-graph", "--ignore-working-copy", "--limit", "1",
                "-r", &src_rev,
                "-T", "change_id.shortest(8) ++ \"\\t\" ++ description.first_line()",
            ])
            .unwrap_or_default();
            let (short, desc) = raw.split_once('\t').unwrap_or((raw.as_str(), ""));
            // Prefix with the base revision's short change id, so auto-named
            // workspaces off different revisions stay distinct and you can see
            // what each sits on. With no description it's just the short id.
            let base = if desc.trim().is_empty() {
                short.to_string()
            } else {
                format!("{short}-{desc}")
            };
            sanitize(&base)
        }
    };
    if workspace_name.is_empty() {
        die("derived workspace name is empty; pass --name NAME");
    }

    // Destination = container/<workspace-name>. Error if it already has files.
    let dest = container.join(&workspace_name);
    if dest.exists() {
        let nonempty = fs::read_dir(&dest)
            .map(|mut it| it.next().is_some())
            .unwrap_or(false);
        if nonempty {
            die(format!(
                "destination already exists and is not empty: {}",
                dest.display()
            ));
        }
    }
    fs::create_dir_all(&container)
        .unwrap_or_else(|e| die(format!("cannot create {}: {e}", container.display())));

    // Create the workspace (cwd-relative so @ resolves in the current workspace).
    let dest_str = dest.to_str().unwrap_or_else(|| die("non-utf8 destination path"));
    let mut add: Vec<&str> = vec!["workspace", "add", "--name", &workspace_name];
    if let Some(r) = &revision {
        add.push("--revision");
        add.push(r);
    }
    add.push(dest_str);

    eprintln!("wts: creating workspace '{workspace_name}' at {}", dest.display());
    let status = Command::new("jj")
        .args(&add)
        .status()
        .unwrap_or_else(|e| die(format!("failed to run jj: {e}")));
    if !status.success() {
        die("jj workspace add failed");
    }

    // Carry over untracked files (AGENTS.override.md, .env, …) that jj doesn't
    // bring into a new workspace itself; opt-in via the `wts.copy` jj config.
    copy_configured_files(&root, &dest);

    // Default action is to cd into the new workspace, mirroring the subdirectory
    // you were in; a `wts.action` script replaces that and is handed the new
    // workspace root instead.
    match action_script() {
        Some(script) => run_action(&script, &dest),
        None => emit_cd(&mirror_subpath(&root, &dest)),
    }
}

fn do_rm(names: Vec<String>) {
    let root = workspace_root();
    let (container, main_repo) = resolve_layout(&root);

    // Each removal carries a jj name (to forget) and a folder (to delete). For
    // named args those are `<name>` and `<container>/<name>` — wts's convention.
    // With no name we target the current workspace: its jj name and its actual
    // root path, both straight from jj, so it's correct even if the two diverge.
    // Only valid inside a worktree — from the main repo there's nothing to remove
    // (and we won't delete the main/default workspace).
    let targets: Vec<(String, PathBuf)> = if names.is_empty() {
        match current_workspace_name(&root) {
            Some(name) => {
                eprintln!("wts: removing current workspace '{name}'");
                vec![(name, root.clone())]
            }
            None => die("no workspace name given and not inside a wts workspace"),
        }
    } else {
        names
            .into_iter()
            .map(|n| {
                let dir = container.join(&n);
                (n, dir)
            })
            .collect()
    };

    let main_str = main_repo
        .to_str()
        .unwrap_or_else(|| die("non-utf8 repo path"));
    let cwd = env::current_dir().ok();
    let mut cwd_removed = false;
    let mut failed = false;

    // Names jj currently knows about, so a typo'd name is a real error rather
    // than a silent no-op (jj's own `workspace forget <missing>` just warns).
    let listing = jj_capture(&["-R", main_str, "workspace", "list"]).unwrap_or_default();
    let known: Vec<&str> = listing
        .lines()
        .filter_map(|l| l.split(':').next().map(str::trim))
        .collect();

    for (name, dir) in &targets {
        let in_jj = known.contains(&name.as_str());
        let on_disk = dir.exists();

        if !in_jj && !on_disk {
            eprintln!("wts: no such workspace: '{name}'");
            failed = true;
            continue;
        }

        // Forget via the main repo so we can drop a workspace even if it's the
        // one we're currently standing in.
        if in_jj {
            let forgot = Command::new("jj")
                .args(["-R", main_str, "workspace", "forget", name])
                .status();
            match forgot {
                Ok(s) if s.success() => {}
                Ok(_) => {
                    eprintln!("wts: 'jj workspace forget {name}' failed; leaving folder in place");
                    failed = true;
                    continue;
                }
                Err(e) => {
                    eprintln!("wts: failed to run jj: {e}");
                    failed = true;
                    continue;
                }
            }
        }

        if on_disk {
            if let Some(c) = &cwd {
                if c.starts_with(dir) {
                    cwd_removed = true;
                }
            }
            match fs::remove_dir_all(dir) {
                Ok(()) => eprintln!("wts: removed workspace '{name}' ({})", dir.display()),
                Err(e) => {
                    eprintln!(
                        "wts: forgot '{name}' but could not delete {}: {e}",
                        dir.display()
                    );
                    failed = true;
                }
            }
        } else {
            eprintln!("wts: forgot '{name}' (no folder at {})", dir.display());
        }
    }

    // If we deleted the folder the shell was sitting in, tell the wrapper to cd
    // back to the main repo so it doesn't strand the shell in a ghost dir.
    if cwd_removed {
        emit_cd(&main_repo);
    }
    if failed {
        exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::mirror_subpath_from;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::{env, fs};

    static N: AtomicUsize = AtomicUsize::new(0);

    /// A fresh, unique temp directory for one test to populate.
    fn scratch() -> PathBuf {
        let id = N.fetch_add(1, Ordering::Relaxed);
        let dir = env::temp_dir().join(format!("wts-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn mirrors_a_subdir_present_in_dest() {
        let base = scratch();
        let (source, dest) = (base.join("src"), base.join("dst"));
        fs::create_dir_all(source.join("a/b")).unwrap();
        fs::create_dir_all(dest.join("a/b")).unwrap();
        assert_eq!(
            mirror_subpath_from(&source, &dest, &source.join("a/b")),
            dest.join("a/b")
        );
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn falls_back_to_dest_when_subdir_missing_in_dest() {
        let base = scratch();
        let (source, dest) = (base.join("src"), base.join("dst"));
        fs::create_dir_all(source.join("a/b")).unwrap();
        fs::create_dir_all(&dest).unwrap(); // dest exists, but dest/a/b does not
        assert_eq!(mirror_subpath_from(&source, &dest, &source.join("a/b")), dest);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn returns_dest_at_source_root() {
        let base = scratch();
        let (source, dest) = (base.join("src"), base.join("dst"));
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&dest).unwrap();
        assert_eq!(mirror_subpath_from(&source, &dest, &source), dest);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn returns_dest_when_cwd_is_outside_source() {
        let base = scratch();
        let (source, dest, other) =
            (base.join("src"), base.join("dst"), base.join("other/deep"));
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::create_dir_all(&other).unwrap();
        assert_eq!(mirror_subpath_from(&source, &dest, &other), dest);
        fs::remove_dir_all(&base).ok();
    }
}
