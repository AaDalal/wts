# wts

Create (and remove) a [jujutsu](https://jj-vcs.github.io/jj/) workspace in a
sibling `<repo>-wts/` folder and `cd` into it, in one command.

```
wts [-r|--revision REV] [-n|--name NAME]   # create + cd into a new workspace
wts rm <name>...                           # forget workspace(s) + delete folder(s)
wts rm                                      # forget + delete the current workspace
```

Configure an [action](#actions) to run in each new workspace — here, opening it
in Claude Code:

![wts creating a workspace and opening it in Claude Code](demo/demo.gif)

## What it does

1. Resolves the current jj repo (workspace) root.
2. Ensures a sibling container folder `<repo-name>-wts/` exists next to it.
3. Creates a jj workspace inside it at `<repo-name>-wts/<workspace-name>`:
   - `--name` if `-n` is given, otherwise derived from the **base revision** (the
     `-r` revision, or `@-` when `-r` is omitted): its short change id, then its
     description — e.g. `qlvrqrmx-fix-the-login-bug`, or just `qlvrqrmx` if the
     revision has no description. The short-id prefix keeps auto-named workspaces
     off different revisions distinct.
   - The name is lowercased, non-alphanumerics become dashes, capped at 32 chars.
   - Errors if that destination already exists and is non-empty.
   - Passes `-r REV` through to `jj workspace add` so the new working copy sits
     on top of that revision.
4. Copies the untracked files listed in `wts.copy` (see below) from the source
   workspace into the new one.
5. Runs the chosen [action](#actions) in the new workspace — the one named
   `default`, or the one you pass with `-a`. The built-in `cd` action moves your
   shell in, mirroring the subdirectory you were in (running from `repo/src/foo`
   lands you in `<new-ws>/src/foo`, falling back to the workspace root if that
   subdirectory isn't there).

Example: from `~/dev/acme`, `wts -n hotfix` creates and enters
`~/dev/acme-wts/hotfix`.

## Actions

Each new workspace runs an **action**. Actions are named and live under
`wts.action.<name>` in jj config; each value is a **script path** or the literal
**`cd`** (a built-in that moves your shell into the workspace). `wts` runs the
action named `default`, or the one you pass with `-a/--action`:

```fish
jj config set --user wts.action.default cd                    # bare `wts` cds you in
jj config set --user wts.action.edit ~/.config/wts/edit.fish  # a named script action
```

```
wts -n hotfix            # runs `default`
wts -n hotfix -a edit    # runs `edit`
wts -n hotfix -a cd      # the built-in `cd`, always available
```

- `wts` with no `-a` runs `default`; if `wts.action.default` isn't set, it
  errors. `-a NAME` for a name that isn't configured errors too, so typos
  surface instead of silently doing nothing.
- Action tables merge across jj config layers, so a `--repo` action **extends**
  your `--user` set (and a per-repo `default` overrides the user one).
- A leading `~/` in a script path expands to `$HOME`.

### Writing a script action

A script carries its own shebang (`#!/usr/bin/env fish`, `bash`, `python3`,
`rust-script`, …) and receives the new workspace directory three ways: as its
first argument (`$1`), as its working directory, and as `$WTS_DIR`. Its stdin,
stdout and stderr are attached to your terminal, so it can be fully interactive
(launch an editor, a shell, `claude`, tmux, …) and anything it spawns starts
inside the workspace. Its exit code becomes `wts`'s.

Example `~/.config/wts/edit.fish` — open the workspace in your editor, then drop
into a shell there:

```fish
#!/usr/bin/env fish
$EDITOR $WTS_DIR &
exec fish
```

### Moving your shell into the new workspace directory

The built-in `cd` action moves your shell into the workspace. A script action
can't do that directly (a child process can't change the calling shell's
directory), but it can opt in by writing the path to `WTS_CD_FILE`, which the
shell wrapper reads and `cd`s into:

```fish
test -n "$WTS_CD_FILE"; and printf '%s\n' "$WTS_DIR" >$WTS_CD_FILE
```

The guard keeps the script working when run outside the wrapper (e.g. `cargo
run`, where `WTS_CD_FILE` is unset).

### Example: open the workspace in cmux

Point cmux at the directory with `--cwd $WTS_DIR` (it doesn't infer the directory
from the calling shell), and title the session after the workspace folder:

```fish
#!/usr/bin/env fish
# ~/.config/wts/cmux.fish — jj config set --user wts.action.cmux ~/.config/wts/cmux.fish
cmux new-workspace --cwd $WTS_DIR --name "(wts) "(basename $WTS_DIR)
```

`wts -n hotfix -a cmux` then opens a cmux workspace rooted at
`…/<repo>-wts/hotfix` titled `(wts) hotfix`. (Set it as `wts.action.default` to
make it the action bare `wts` runs.)

## Copying untracked files

jj carries your **tracked** files into a new workspace, but ignored/untracked
ones (`AGENTS.override.md`, `.env`, local tool config) stay behind. Declare glob
patterns in the `wts.copy` jj config **table** and `wts` re-materializes the
matching files from the source workspace into the new one. Each entry key is
just a label; the string value is the glob:

```fish
jj config set --user wts.copy.agents AGENTS.override.md   # applies everywhere
jj config set --user wts.copy.env '.env*'
jj config set --repo wts.copy.local CLAUDE.local.md       # adds to the above in this repo
```

- A **table** (not an array) is required on purpose: jj *merges* config tables
  across layers, so a `--repo` entry **extends** your `--user` set rather than
  replacing it. (jj replaces arrays wholesale, which is why they aren't used.)
- Patterns are globs (`*`, `**`, `?`, `[...]`) resolved relative to the source
  workspace root; matched directories are copied recursively.
- Unset by default: nothing is copied unless you opt in. Missing matches are
  skipped silently; a copy error is a warning, never fatal.
- Inspect the rules in force for a repo with `jj config get wts.copy`, or
  `jj config list wts.copy` to see each entry.

## Install

The `wts` binary does the work and writes the directory to `cd` into to a
scratch file (named via `WTS_CD_FILE`); the fish function reads it and performs
the `cd` (a child process can't change the parent shell's directory). Routing
the `cd` through a file rather than stdout keeps the terminal free for an
action script to run interactively. Requires `jj` on `PATH`.

With [`just`](https://github.com/casey/just): `just install` (or `just
reinstall` to redo it). By hand, from the repo root:

```fish
# from the repo root ($PWD is its absolute path; symlink targets must be absolute)
cargo install --path $PWD                          # builds + installs `wts` to ~/.cargo/bin
ln -s $PWD/wts.fish ~/.config/fish/conf.d/wts.fish
ln -s $PWD/completions/wts.fish ~/.config/fish/completions/wts.fish
```

The fish function shadows the binary and reaches it via `command wts`, so make
sure `~/.cargo/bin` is on `PATH`.

Completions: `wts rm <TAB>` lists the repo's workspaces (with their commit
descriptions, `default` excluded) and `wts -r <TAB>` lists bookmarks.

## Uninstall

`just uninstall`, or reverse the three install steps by hand — drop the binary
and remove the two symlinks:

```fish
cargo uninstall wts                               # removes `wts` from ~/.cargo/bin
rm ~/.config/fish/conf.d/wts.fish                 # the shell function
rm ~/.config/fish/completions/wts.fish            # the completions
```

This leaves your jj config untouched. To also forget the `wts.*` settings, unset
each key (`wts.action.*` and `wts.copy.*` are tables, so remove their entries
individually):

```fish
jj config unset --user wts.action.default         # one per wts.action entry
jj config unset --user wts.copy.agents            # one per wts.copy entry
jj config unset --user wts.copy.env
```

`jj config unset wts.action` (or `wts.copy`) won't work — jj refuses to delete a
whole table at once ("Would delete entire table"), so you remove the entries one
by one. To wipe the lot in a single step instead, open the config and delete the
`[wts.action]` and `[wts.copy]` blocks by hand:

```fish
jj config edit --user                             # or --repo for repo-local settings
```

Already-created `<repo>-wts/<name>` workspaces are unaffected; remove them with
`wts rm <name>` *before* uninstalling, or by hand afterward with
`jj workspace forget <name>` plus deleting the folder.

## Develop

```
cargo build          # debug build at target/debug/wts
cargo run -- -n foo  # run without installing (no cd; prints the path)
```

## Removing workspaces

```
wts rm <name>            # jj workspace forget <name> + rm -rf <repo>-wts/<name>
wts rm alpha beta        # several at once
wts rm                   # remove the workspace you're currently in
```

`rm` works from the main repo or from inside another workspace, and even from
inside the one you're deleting — when it removes the folder you were standing
in, it prints the main repo path so the shell function cd's you back there. A
name that's neither a known workspace nor a folder on disk is an error.

With no name, `wts rm` removes the workspace you're standing in: it asks jj for
the current workspace's name and forgets that, and deletes the current workspace
root, so it stays correct even if the folder name and the jj workspace name have
drifted apart. It's only valid from inside a `<repo>-wts/<name>` worktree — run
from the main repo it errors rather than touching the default workspace, so you
can't accidentally nuke the repo you launched from.

You can still do it by hand with plain jj:

```
jj workspace list
jj workspace forget <name>
```
