//! wts — create or remove jj workspaces in a sibling `<repo>-wts/` folder.
//!
//! With no subcommand it creates a workspace and prints its absolute path as the
//! ONLY stdout line; `wts rm <name>...` forgets workspaces and deletes their
//! folders. All human-facing messages go to stderr. The `wts` shell function
//! captures stdout and `cd`s into it — for create that's the new workspace, and
//! for `rm` it's the main repo when you just deleted the folder you were in (a
//! child process cannot change the parent shell's directory itself).

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
        /// Workspace name(s) to remove
        #[arg(required = true)]
        names: Vec<String>,
    },
}

fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("wts: error: {}", msg.as_ref());
    exit(1);
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
            // "parent revision" we name after is @-.
            let src_rev = revision.clone().unwrap_or_else(|| "@-".to_string());
            let desc = jj_capture(&[
                "log", "--no-graph", "--ignore-working-copy", "--limit", "1",
                "-r", &src_rev, "-T", "description.first_line()",
            ])
            .unwrap_or_default();
            let base = if desc.trim().is_empty() {
                jj_capture(&[
                    "log", "--no-graph", "--ignore-working-copy", "--limit", "1",
                    "-r", &src_rev, "-T", "change_id.shortest(8)",
                ])
                .unwrap_or_default()
            } else {
                desc
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

    // Emit the path for the shell wrapper to cd into.
    println!("{}", dest.display());
}

fn do_rm(names: Vec<String>) {
    let root = workspace_root();
    let (container, main_repo) = resolve_layout(&root);
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

    for name in &names {
        let dir = container.join(name);
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
                if c.starts_with(&dir) {
                    cwd_removed = true;
                }
            }
            match fs::remove_dir_all(&dir) {
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
        println!("{}", main_repo.display());
    }
    if failed {
        exit(1);
    }
}
