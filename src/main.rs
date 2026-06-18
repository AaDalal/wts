//! wts: create or remove workspaces in a sibling `<repo>-wts/` folder.
//!
//! wts works on top of either [jujutsu](https://jj-vcs.github.io/jj/) (a jj
//! workspace) or plain [git](https://git-scm.com/) (a git worktree); it detects
//! which at startup (see [`detect_backend`]). With no subcommand it creates a
//! workspace; `wts rm <name>...` removes workspaces and deletes their folders.
//! Human-facing messages go to stderr.
//!
//! A child process cannot change the parent shell's directory, so when wts wants
//! the shell to cd somewhere it writes the target path to the file named by the
//! `WTS_CD_FILE` env var (set by the `wts` shell function), which then cd's
//! there: into the new workspace on create, or back to the main repo after `rm`
//! deletes the folder you were standing in. Run without `WTS_CD_FILE` (e.g. a
//! direct `cargo run`) there's no shell to cd, so the path just appears on
//! stderr with the rest of the diagnostics.
//!
//! What a new workspace does once created is its **action**: `-a NAME`, else
//! the action named `default`. Actions are configured under `wts.action.<name>`
//! (a script path, or the literal `cd` for the built-in cd; see `resolve_action`).
//! Config lives in jj config (TOML) or git config (INI) depending on the backend.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use clap::{Parser, Subcommand};

/// Which VCS backs the current repo. Selected once per run by [`detect_backend`]
/// and threaded through the create/remove logic; jj and git differ in how
/// workspaces are created, named, listed, and configured.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Backend {
    Jj,
    Git,
}

#[derive(Parser)]
#[command(
    name = "wts",
    about = "Create (or remove) a jj/git workspace in a sibling <repo>-wts/ folder"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Base revision/ref the new workspace sits on (jj: default same parents as
    /// @; git: default HEAD, or the `wts.baseRef` config)
    #[arg(short, long)]
    revision: Option<String>,

    /// Workspace name; if omitted, derived from the base revision's
    /// description/subject (sanitized: lowercase, dashes, <=32 chars)
    #[arg(short, long)]
    name: Option<String>,

    /// Action to run in the new workspace (a `wts.action.<name>` entry, or the
    /// built-in `cd`); defaults to the action named `default`
    #[arg(short, long)]
    action: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Remove workspaces: forget/remove the worktree + delete each `<repo>-wts/<name>` folder
    Rm {
        /// Workspace name(s) to remove; omit to remove the current workspace
        /// (only valid when run from inside a `<repo>-wts/<name>` workspace)
        names: Vec<String>,
    },
    /// Print the shell integration (the `wts` function); e.g. `wts init fish | source`
    Init {
        /// Shell to emit integration for: fish, bash, or zsh
        shell: String,
    },
    /// Print shell completions; e.g. `wts completions fish | source`
    Completions {
        /// Shell to emit completions for: fish, bash, or zsh
        shell: String,
    },
}

fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("wts: error: {}", msg.as_ref());
    exit(1);
}

/// Tell the shell wrapper where to cd by writing the path to the scratch file it
/// names in `WTS_CD_FILE`. With no `WTS_CD_FILE` (e.g. a direct `cargo run`,
/// outside the wrapper) there's nothing to cd, so this is a no-op: the path is
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

/// What running `wts` does in the new workspace once it's created.
enum Action {
    /// Built-in: cd the shell into the workspace (mirroring the subdirectory).
    Cd,
    /// Run a user script (path already tilde-expanded).
    Script(String),
}

/// Resolve action `name` to what it runs. A `wts.action.<name>` config entry
/// wins; its value is either the literal `cd` (the built-in) or a script path.
/// `cd` is also a built-in available without any config. Unknown names yield
/// `None`. Config layers merge (jj merges tables, git merges its config files),
/// so `--repo`/`--local` entries extend the user-level set.
fn resolve_action(backend: Backend, name: &str) -> Option<Action> {
    let key = format!("wts.action.{name}");
    let configured = match backend {
        Backend::Jj => jj_capture(&["config", "get", &key]).ok(),
        Backend::Git => git_capture(&["config", "--get", &key]).ok(),
    }
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());
    match configured.as_deref() {
        Some("cd") => Some(Action::Cd),
        Some(path) => Some(Action::Script(expand_tilde(path))),
        None if name == "cd" => Some(Action::Cd),
        None => None,
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

/// Run an action `script` in the new workspace. The script carries its own
/// shebang (fish, bash, python, rust-script, …) and is run with the workspace as
/// both its working directory and its sole argument, and with that path also
/// exported as `$WTS_DIR`. Stdio is inherited so interactive actions (opening an
/// editor, starting a shell) work. The action's exit code becomes ours; the
/// workspace was already created, so this only reports how the action fared.
fn run_action(script: &str, dest: &Path) {
    eprintln!("wts: running action ({script})");
    let status = Command::new(script)
        .arg(dest)
        .env("WTS_DIR", dest)
        .current_dir(dest)
        .status()
        .unwrap_or_else(|e| die(format!("failed to run action '{script}': {e}")));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

/// Run a jj command and return trimmed stdout, or the trimmed stderr as Err.
fn jj_capture(args: &[&str]) -> Result<String, String> {
    capture("jj", args)
}

/// Run a git command and return trimmed stdout, or the trimmed stderr as Err.
/// git uses exit code 1 to mean "config key not set" (and similar absences), so
/// callers reading optional config treat any Err as "unset".
fn git_capture(args: &[&str]) -> Result<String, String> {
    capture("git", args)
}

fn capture(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run {program}: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Read a scalar git config string, or `default` if unset/empty. git-only knob;
/// the jj backend keeps its values hardcoded so its behavior is unchanged.
fn git_config_str(key: &str, default: &str) -> String {
    git_capture(&["config", "--get", key])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Read a boolean git config knob (`--type=bool` normalizes true/yes/on/1), or
/// `default` if unset.
fn git_config_bool(key: &str, default: bool) -> bool {
    match git_capture(&["config", "--type=bool", "--get", key]) {
        Ok(s) => s.trim() == "true",
        Err(_) => default,
    }
}

/// The suffix for the sibling container dir (`<repo><suffix>`). git exposes this
/// as `wts.containerSuffix`; jj keeps the historical `-wts`.
fn container_suffix(backend: Backend) -> String {
    match backend {
        Backend::Jj => "-wts".to_string(),
        Backend::Git => git_config_str("wts.containerSuffix", "-wts"),
    }
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

/// Copy the files matched by the `wts.copy` config from the source workspace into
/// the freshly-created one. These are the untracked/ignored files (e.g.
/// `AGENTS.override.md`, `.env`) that neither jj nor git carry into a new
/// workspace on their own. Unset config copies nothing; missing matches are
/// skipped silently; a copy failure is a warning, never fatal.
fn copy_configured_files(backend: Backend, source: &Path, dest: &Path) {
    let patterns = copy_patterns(backend);
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

/// Glob patterns to copy, read from the `wts.copy` config. Both backends are
/// designed so a per-repo entry *extends* the user-level set rather than
/// replacing it. The returned list is deduped and sorted for a deterministic
/// copy order regardless of layer/merge.
///
/// - jj: the `wts.copy.<label> = "<glob>"` TOML table (jj merges tables across
///   config layers). `jj config list` prints valid TOML, so we parse it.
/// - git: the multi-valued `wts.copy = "<glob>"` key (git's idiomatic "a list
///   local extends"), read with `git config --get-all --null wts.copy`.
fn copy_patterns(backend: Backend) -> Vec<String> {
    let mut pats = match backend {
        Backend::Jj => copy_patterns_jj(),
        Backend::Git => copy_patterns_git(),
    };
    pats.sort();
    pats.dedup();
    pats
}

fn copy_patterns_jj() -> Vec<String> {
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
        Some(toml::Value::Table(t)) => t
            .values()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    }
}

fn copy_patterns_git() -> Vec<String> {
    // `--null` separates values with NUL so globs/paths containing spaces stay
    // intact; an unset key exits non-zero, which we treat as "no patterns".
    let out = match Command::new("git")
        .args(["config", "--get-all", "--null", "wts.copy"])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return vec![],
    };
    String::from_utf8_lossy(&out)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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

/// Detect the VCS backend and locate the repo root. jj wins in a colocated repo
/// (a `.jj` dir at the root), otherwise we use git. Dies if we're in neither.
fn detect_backend() -> (Backend, PathBuf) {
    // jj first: in a colocated jj+git repo `.jj` is present, so jj wins,
    // matching wts's historical behavior.
    if let Ok(root) = jj_capture(&["workspace", "root"]) {
        let root = PathBuf::from(root);
        if root.join(".jj").is_dir() {
            return (Backend::Jj, root);
        }
    }
    if let Ok(top) = git_capture(&["rev-parse", "--show-toplevel"]) {
        return (Backend::Git, PathBuf::from(top));
    }
    die("not inside a jj or git repo");
}

/// Resolve the `<repo><suffix>` container and the main repo path, whether `root`
/// is the main repo itself or one of its `<repo><suffix>/<name>` workspaces.
fn resolve_layout(root: &Path, suffix: &str) -> (PathBuf, PathBuf) {
    let parent = root
        .parent()
        .unwrap_or_else(|| die("repo root has no parent directory"));
    let base = root
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_else(|| die("cannot determine repo name from root path"));
    // If the parent dir is itself a `<repo><suffix>` container, we're inside a
    // workspace: the container is the parent, the main repo its sibling.
    if let Some(pb) = parent.file_name().and_then(OsStr::to_str) {
        if let Some(repo_name) = pb.strip_suffix(suffix) {
            let main_repo = parent
                .parent()
                .map(|g| g.join(repo_name))
                .unwrap_or_else(|| die("cannot resolve main repo path"));
            return (parent.to_path_buf(), main_repo);
        }
    }
    (parent.join(format!("{base}{suffix}")), root.to_path_buf())
}

/// The name of the workspace we're currently in, if it's a `<repo><suffix>/<name>`
/// worktree rather than the main repo. Returns None from the main repo (so a
/// no-name `rm` there can't target the main/default workspace).
///
/// For jj we ask which workspace owns the current working copy (jj, not the
/// folder name, is the source of truth): wts creates each workspace with its
/// folder name as the jj name, but the two can diverge (e.g. `jj workspace
/// rename`), and `jj workspace forget` needs the jj name. For git a worktree has
/// no identity beyond its directory, so the folder name *is* the name.
fn current_workspace_name(backend: Backend, root: &Path, suffix: &str) -> Option<String> {
    // Gate on actually being inside a `<suffix>` container; this is what keeps
    // the main workspace off-limits to a no-name `rm`.
    let parent = root.parent()?;
    parent.file_name().and_then(OsStr::to_str)?.strip_suffix(suffix)?;

    let folder = || root.file_name().and_then(OsStr::to_str).map(str::to_string);

    match backend {
        Backend::Git => folder(),
        Backend::Jj => {
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
                // Exactly one current working copy is the unambiguous case;
                // anything else (none, or several sharing a commit) falls back
                // to the folder.
                (names.len() == 1).then(|| names.remove(0))
            });
            from_jj.or_else(folder)
        }
    }
}

/// The names of the workspaces the backend currently knows about (used so a
/// typo'd `rm <name>` is a real error rather than a silent no-op). For git this
/// is the linked worktrees' folder names; the main worktree is excluded.
fn known_workspaces(backend: Backend, main_str: &str) -> Vec<String> {
    match backend {
        Backend::Jj => {
            let listing =
                jj_capture(&["-R", main_str, "workspace", "list"]).unwrap_or_default();
            listing
                .lines()
                .filter_map(|l| l.split(':').next().map(str::trim))
                .map(str::to_string)
                .collect()
        }
        Backend::Git => {
            let porcelain =
                git_capture(&["-C", main_str, "worktree", "list", "--porcelain"]).unwrap_or_default();
            linked_worktree_names(&porcelain)
        }
    }
}

/// Parse `git worktree list --porcelain` into the basenames of the *linked*
/// worktrees, skipping the first record (always the main worktree).
fn linked_worktree_names(porcelain: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut first = true;
    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if first {
                first = false;
                continue;
            }
            if let Some(base) = Path::new(path).file_name().and_then(OsStr::to_str) {
                names.push(base.to_string());
            }
        }
    }
    names
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::Rm { names }) => {
            let (backend, root) = detect_backend();
            do_rm(backend, root, names);
        }
        Some(Cmd::Init { shell }) => emit_init(&shell),
        Some(Cmd::Completions { shell }) => emit_completions(&shell),
        None => {
            let (backend, root) = detect_backend();
            do_create(backend, root, cli.revision, cli.name, cli.action);
        }
    }
}

/// Print the embedded shell-integration function for `shell`. The files are
/// baked in at build time so a `cargo install`ed binary carries its own shell
/// integration (no separate download).
fn emit_init(shell: &str) {
    let contents = match shell {
        "fish" => include_str!("../wts.fish"),
        "bash" => include_str!("../wts.bash"),
        "zsh" => include_str!("../wts.zsh"),
        other => die(format!(
            "unsupported shell '{other}'; supported: fish, bash, zsh"
        )),
    };
    print!("{contents}");
}

/// Print the embedded completions for `shell`.
fn emit_completions(shell: &str) {
    let contents = match shell {
        "fish" => include_str!("../completions/wts.fish"),
        "bash" => include_str!("../completions/wts.bash"),
        "zsh" => include_str!("../completions/wts.zsh"),
        other => die(format!(
            "unsupported shell '{other}'; supported: fish, bash, zsh"
        )),
    };
    print!("{contents}");
}

fn do_create(
    backend: Backend,
    root: PathBuf,
    revision: Option<String>,
    name: Option<String>,
    action: Option<String>,
) {
    let suffix = container_suffix(backend);
    let (container, _main_repo) = resolve_layout(&root, &suffix);

    // Resolve the action up front so an unknown one fails before we create
    // anything. `-a NAME`, else the action named `default`.
    let requested = action.as_deref().unwrap_or("default");
    let act = resolve_action(backend, requested).unwrap_or_else(|| {
        if action.is_some() {
            die(format!(
                "no action '{requested}' configured; set wts.action.{requested}, or use the built-in `cd`"
            ))
        } else {
            die(
                "no default action configured; pass -a/--action NAME, or set one with \
                 e.g. `jj config set --user wts.action.default cd` or `git config wts.action.default cd`",
            )
        }
    });

    // For git, the base ref the new worktree checks out (and we name after):
    // explicit --revision, else the `wts.baseRef` config (default HEAD). jj
    // derives its base differently (see below), so this is only used for git.
    let git_base = revision
        .clone()
        .unwrap_or_else(|| git_config_str("wts.baseRef", "HEAD"));

    // Workspace name: explicit --name, else derived from the base revision.
    let workspace_name = match &name {
        Some(n) => sanitize(n),
        None => match backend {
            Backend::Jj => {
                // No --revision => the new working copy shares @'s parents, so
                // the base revision we name after is @-.
                let src_rev = revision.clone().unwrap_or_else(|| "@-".to_string());
                // Grab the short change id and the description's first line in
                // one shot (tab-separated) so we can prefix the name.
                let raw = jj_capture(&[
                    "log", "--no-graph", "--ignore-working-copy", "--limit", "1",
                    "-r", &src_rev,
                    "-T", "change_id.shortest(8) ++ \"\\t\" ++ description.first_line()",
                ])
                .unwrap_or_default();
                let (short, desc) = raw.split_once('\t').unwrap_or((raw.as_str(), ""));
                sanitize(&name_from(short, desc))
            }
            Backend::Git => {
                // The short commit hash + subject of the base ref; mirrors jj's
                // change-id + description naming.
                let short = git_capture(&["rev-parse", "--short", &git_base]).unwrap_or_default();
                let subj = git_capture(&["log", "-1", "--format=%s", &git_base]).unwrap_or_default();
                sanitize(&name_from(&short, &subj))
            }
        },
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

    let dest_str = dest.to_str().unwrap_or_else(|| die("non-utf8 destination path"));

    eprintln!("wts: creating workspace '{workspace_name}' at {}", dest.display());
    match backend {
        Backend::Jj => {
            // cwd-relative so @ resolves in the current workspace.
            let mut add: Vec<&str> = vec!["workspace", "add", "--name", &workspace_name];
            if let Some(r) = &revision {
                add.push("--revision");
                add.push(r);
            }
            add.push(dest_str);
            let status = Command::new("jj")
                .args(&add)
                .status()
                .unwrap_or_else(|e| die(format!("failed to run jj: {e}")));
            if !status.success() {
                die("jj workspace add failed");
            }
        }
        Backend::Git => {
            // By default create a branch named after the worktree (optionally
            // prefixed), the git equivalent of jj's own working-copy commit: a
            // place to accumulate work. It also sidesteps git's refusal to check
            // out a branch already live in another worktree. `wts.createBranch
            // false` checks out a detached HEAD instead.
            let create_branch = git_config_bool("wts.createBranch", true);
            let branch = format!("{}{}", git_config_str("wts.branchPrefix", ""), workspace_name);
            let mut add: Vec<&str> = vec!["worktree", "add"];
            if create_branch {
                add.push("-b");
                add.push(&branch);
            } else {
                add.push("--detach");
            }
            add.push(dest_str);
            add.push(&git_base);
            let status = Command::new("git")
                .args(&add)
                .status()
                .unwrap_or_else(|e| die(format!("failed to run git: {e}")));
            if !status.success() {
                // git already printed the underlying reason. Add a hint for the
                // cases its wording doesn't make obvious: most often the base ref
                // doesn't resolve to a commit (an empty repo's unborn HEAD).
                let base_ok = git_capture(&["rev-parse", "--verify", "--quiet", &format!("{git_base}^{{commit}}")])
                    .is_ok();
                if !base_ok {
                    if git_base == "HEAD" {
                        die("the repo has no commits yet — make an initial commit \
                             (e.g. `git commit --allow-empty -m init`) before creating a worktree");
                    }
                    die(format!(
                        "base ref '{git_base}' doesn't resolve to a commit; pass -r with a valid ref"
                    ));
                }
                die("git worktree add failed (see git's message above)");
            }
        }
    }

    // Carry over untracked files (AGENTS.override.md, .env, …) that a new
    // workspace doesn't get on its own; opt-in via the `wts.copy` config.
    copy_configured_files(backend, &root, &dest);

    // Run the action resolved above. The built-in `cd` cds the shell in
    // (mirroring the subdirectory you were in); a script is handed the new
    // workspace root instead.
    match act {
        Action::Cd => emit_cd(&mirror_subpath(&root, &dest)),
        Action::Script(script) => run_action(&script, &dest),
    }
}

/// Compose a workspace name from a short revision id and a description/subject:
/// `<id>-<desc>`, or just `<id>` when the description is empty.
fn name_from(short: &str, desc: &str) -> String {
    if desc.trim().is_empty() {
        short.to_string()
    } else {
        format!("{short}-{desc}")
    }
}

/// Resolve a `wts rm` argument to a `(name, folder)` target. The argument is
/// tried first as a worktree *name* (a plain `<container>/<name>` entry), then
/// as a *path* (relative to cwd, or absolute) that must resolve to a folder
/// sitting directly under the container. Returns None if it's neither — which is
/// what stops `.`, `..`, or a stray path that lands on the container or the main
/// repo from being mistaken for a removable workspace.
fn resolve_rm_target(
    container: &Path,
    known: &[String],
    cwd: Option<&Path>,
    arg: &str,
) -> Option<(String, PathBuf)> {
    // Name case: a plain component (no separators, not `.`/`..`) that names a
    // worktree under the container, registered or just present on disk.
    let plain = !arg.is_empty()
        && arg != "."
        && arg != ".."
        && !arg.contains('/')
        && !arg.contains(std::path::MAIN_SEPARATOR);
    if plain {
        let dir = container.join(arg);
        if known.iter().any(|k| k == arg) || dir.exists() {
            return Some((arg.to_string(), dir));
        }
    }
    // Path case: resolve relative to cwd (or absolute), and accept it only if it
    // points at a folder directly inside the container. canonicalize requires
    // the path to exist, so a typo'd name falls through to None here.
    let raw = Path::new(arg);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd?.join(raw)
    };
    let canon = fs::canonicalize(&joined).ok()?;
    let container_canon = fs::canonicalize(container).ok()?;
    if canon.parent()? == container_canon {
        let name = canon.file_name().and_then(OsStr::to_str)?.to_string();
        return Some((name, canon));
    }
    None
}

fn do_rm(backend: Backend, root: PathBuf, names: Vec<String>) {
    let suffix = container_suffix(backend);
    let (container, main_repo) = resolve_layout(&root, &suffix);

    let main_str = main_repo
        .to_str()
        .unwrap_or_else(|| die("non-utf8 repo path"));
    let cwd = env::current_dir().ok();
    let mut cwd_removed = false;
    let mut failed = false;

    // Names the backend currently knows about, so a typo'd name is a real error.
    let known = known_workspaces(backend, main_str);

    // With no argument we target the current workspace (only valid inside a
    // worktree; from the main repo there's nothing to remove). Otherwise each
    // argument is resolved first as a worktree name, then as a path to a
    // worktree folder (see resolve_rm_target).
    let targets: Vec<(String, PathBuf)> = if names.is_empty() {
        match current_workspace_name(backend, &root, &suffix) {
            Some(name) => {
                eprintln!("wts: removing current workspace '{name}'");
                vec![(name, root.clone())]
            }
            None => die("no workspace name given and not inside a wts workspace"),
        }
    } else {
        let mut out = Vec::new();
        for arg in &names {
            match resolve_rm_target(&container, &known, cwd.as_deref(), arg) {
                Some(target) => out.push(target),
                None => {
                    eprintln!("wts: no such workspace: '{arg}'");
                    failed = true;
                }
            }
        }
        out
    };

    for (name, dir) in &targets {
        // `default` is jj's main workspace (the repo itself), not a wts-managed
        // sibling. Refuse it so `wts rm default` can't detach the main repo.
        if name == "default" {
            eprintln!("wts: refusing to remove the 'default' (main) workspace");
            failed = true;
            continue;
        }

        let in_vcs = known.iter().any(|k| k == name);
        let on_disk = dir.exists();

        if !in_vcs && !on_disk {
            eprintln!("wts: no such workspace: '{name}'");
            failed = true;
            continue;
        }

        let removing_cwd = cwd.as_deref().is_some_and(|c| c.starts_with(dir));

        match backend {
            Backend::Jj => {
                if remove_jj(main_str, name, dir, in_vcs, on_disk, removing_cwd, &mut cwd_removed) {
                    failed = true;
                }
            }
            Backend::Git => {
                if remove_git(main_str, name, dir, removing_cwd, &mut cwd_removed) {
                    failed = true;
                }
            }
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

/// jj removal: `jj workspace forget` (via the main repo, so we can drop the one
/// we're standing in) then delete the folder. Returns true on failure.
fn remove_jj(
    main_str: &str,
    name: &str,
    dir: &Path,
    in_vcs: bool,
    on_disk: bool,
    removing_cwd: bool,
    cwd_removed: &mut bool,
) -> bool {
    // Forget is keyed on the jj name; a folder with no registered workspace just
    // gets deleted below.
    if in_vcs {
        match Command::new("jj")
            .args(["-R", main_str, "workspace", "forget", name])
            .status()
        {
            Ok(s) if s.success() => {}
            Ok(_) => {
                eprintln!("wts: 'jj workspace forget {name}' failed; leaving folder in place");
                return true;
            }
            Err(e) => {
                eprintln!("wts: failed to run jj: {e}");
                return true;
            }
        }
    }

    if on_disk {
        match fs::remove_dir_all(dir) {
            Ok(()) => {
                if removing_cwd {
                    *cwd_removed = true;
                }
                eprintln!("wts: removed workspace '{name}' ({})", dir.display());
            }
            Err(e) => {
                eprintln!("wts: forgot '{name}' but could not delete {}: {e}", dir.display());
                return true;
            }
        }
    } else {
        eprintln!("wts: forgot '{name}' (no folder at {})", dir.display());
    }
    false
}

/// git removal: `git worktree remove --force` (forgets + deletes the folder in
/// one step) then delete the wts-created branch. Returns true on failure.
fn remove_git(
    main_str: &str,
    name: &str,
    dir: &Path,
    removing_cwd: bool,
    cwd_removed: &mut bool,
) -> bool {
    let removed = Command::new("git")
        .args(["-C", main_str, "worktree", "remove", "--force"])
        .arg(dir)
        .output();
    let ok = matches!(&removed, Ok(o) if o.status.success());

    if !ok {
        // The folder may have been deleted out from under git; prune the stale
        // registration, then clean up any folder git left behind.
        let _ = Command::new("git")
            .args(["-C", main_str, "worktree", "prune"])
            .output();
        if dir.exists() {
            if let Err(e) = fs::remove_dir_all(dir) {
                eprintln!("wts: could not delete {}: {e}", dir.display());
                return true;
            }
        }
    }

    if removing_cwd {
        *cwd_removed = true;
    }

    // Delete the branch wts created for this worktree. Silent and best-effort:
    // it's just cleanup, and a detached-HEAD worktree has no such branch.
    let branch = format!("{}{}", git_config_str("wts.branchPrefix", ""), name);
    let _ = Command::new("git")
        .args(["-C", main_str, "branch", "-D", &branch])
        .output();

    eprintln!("wts: removed workspace '{name}' ({})", dir.display());
    false
}

#[cfg(test)]
mod tests {
    use super::{linked_worktree_names, mirror_subpath_from, name_from, sanitize};
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

    #[test]
    fn sanitize_lowercases_and_dashes() {
        assert_eq!(sanitize("Hello World!"), "hello-world");
        assert_eq!(sanitize("  --Foo__Bar--  "), "foo-bar");
        assert_eq!(sanitize("a/b/c"), "a-b-c");
    }

    #[test]
    fn sanitize_caps_at_32_chars() {
        let s = sanitize(&"x".repeat(40));
        assert_eq!(s.len(), 32);
    }

    #[test]
    fn name_from_joins_or_drops_empty_desc() {
        assert_eq!(name_from("abc123", "fix the bug"), "abc123-fix the bug");
        assert_eq!(name_from("abc123", "   "), "abc123");
        assert_eq!(name_from("abc123", ""), "abc123");
    }

    #[test]
    fn linked_worktree_names_skips_main() {
        let porcelain = "\
worktree /home/me/repo
HEAD aaaa
branch refs/heads/main

worktree /home/me/repo-wts/feature-a
HEAD bbbb
branch refs/heads/feature-a

worktree /home/me/repo-wts/detached-one
HEAD cccc
detached
";
        assert_eq!(
            linked_worktree_names(porcelain),
            vec!["feature-a".to_string(), "detached-one".to_string()]
        );
    }

    #[test]
    fn linked_worktree_names_empty_when_only_main() {
        let porcelain = "worktree /home/me/repo\nHEAD aaaa\nbranch refs/heads/main\n";
        assert!(linked_worktree_names(porcelain).is_empty());
    }
}
