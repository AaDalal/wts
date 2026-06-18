# wts

Create (and remove) a [jujutsu](https://jj-vcs.github.io/jj/) workspace in a
sibling `<repo>-wts/` folder and `cd` into it, in one command.

```
wts [-r|--revision REV] [-n|--name NAME]   # create + cd into a new workspace
wts rm <name>...                           # forget workspace(s) + delete folder(s)
wts rm                                      # forget + delete the current workspace
```

Set a [`wts.action`](#customizing-the-action) to run anything in the new
workspace — here, opening it in Claude Code:

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
5. Runs the **action**: by default, `cd`s into the new workspace — mirroring the
   subdirectory you were in, so running from `repo/src/foo` lands you in
   `<new-ws>/src/foo` (falling back to the workspace root if that subdirectory
   isn't there). Or, if you've set `wts.action`, runs your script there instead
   (see below).

Example: from `~/Documents/dev/sail`, `wts -n hotfix` creates and enters
`~/Documents/dev/sail-wts/hotfix`.

## Customizing the action

By default, creating a workspace just `cd`s your shell into it. Set the
`wts.action` jj config to an **executable script** to run something else
instead — open an editor, start a shell or tmux session, kick off an agent, etc:

```fish
jj config set --user wts.action ~/.config/wts/on-create.fish   # everywhere
jj config set --repo wts.action ./.wts-action                  # just this repo
```

The script carries its own shebang (`#!/usr/bin/env fish`, `bash`, `python3`,
`rust-script`, …) and is run with the **new workspace directory** delivered three
ways, so use whichever is convenient:

- as its first argument (`$1`),
- as its working directory (`$PWD`),
- as the `WTS_DIR` environment variable.

A leading `~/` in the configured path expands to `$HOME`. The script runs **with
the new workspace as its working directory**, and its stdin, stdout and stderr
are attached to your terminal — so it can be fully interactive (launch an editor,
a shell, `claude`, tmux, …) and anything it spawns starts inside the workspace
without a `cd`. Its exit code becomes `wts`'s exit code (the workspace is already
created either way). Unset (the default) keeps the plain `cd`.

### Moving your shell into the new workspace directory

Setting `wts.action` turns off the default behavior of `cd`-ing your shell into
the new workspace directory. A `cd` inside the action only changes the action's
own directory, not the terminal you ran `wts` from. To end up in the new
workspace, do one of:

- Start a shell or program from the action — it already runs in the workspace
  (see the editor example below), so you're there for as long as it's open.
- Write `$WTS_DIR` to `WTS_CD_FILE` to make your terminal `cd` in, just like the
  default does. The shell wrapper passes that variable to the action; guard on it
  so the script still works when run directly (e.g. `cargo run`):

  ```fish
  test -n "$WTS_CD_FILE"; and printf '%s\n' "$WTS_DIR" >$WTS_CD_FILE
  ```

Example `~/.config/wts/on-create.fish` that opens the workspace in your editor
and then drops you into a shell there (no `cd` needed — the action already runs
in `$WTS_DIR`):

```fish
#!/usr/bin/env fish
$EDITOR $WTS_DIR &
exec fish
```

### Example: open the workspace in cmux

Hand each new workspace to cmux as its own session. Point cmux at the directory
with `--cwd $WTS_DIR` (it does **not** infer the directory from the calling
shell's cwd, so passing the path explicitly is what makes the session open in
the right place). The session title is derived from the workspace folder and
tagged so cmux sessions created this way are easy to spot:

```fish
#!/usr/bin/env fish
# ~/.config/wts/cmux.fish — set via: jj config set --user wts.action ~/.config/wts/cmux.fish
cmux new-workspace --cwd $WTS_DIR --name "(wts) "(basename $WTS_DIR)
```

`wts -n hotfix` then opens a cmux workspace rooted at `…/<repo>-wts/hotfix` with
the title `(wts) hotfix`. (The simpler `cmux $WTS_DIR` also opens the directory
in a new workspace, but doesn't let you set the title.)

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
the `cd` through a file rather than stdout keeps the terminal free for a
`wts.action` script to run interactively. Requires `jj` on `PATH`.

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
each key:

```fish
jj config unset --user wts.action                 # and any --repo you set
jj config unset --user wts.copy.agents            # one per wts.copy entry
jj config unset --user wts.copy.env
```

`jj config unset wts.copy` won't work — jj refuses to delete a whole table at
once ("Would delete entire table"), so you remove the `wts.copy.*` entries one
by one. To wipe the lot in a single step instead, open the config and delete the
`[wts.copy]` block (and the `wts.action` line) by hand:

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
