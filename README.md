# wts

Create (and remove) a [jujutsu](https://jj-vcs.github.io/jj/) workspace in a
sibling `<repo>-wts/` folder and `cd` into it, in one command.

```
wts [-r|--revision REV] [-n|--name NAME]   # create + cd into a new workspace
wts rm <name>...                           # forget workspace(s) + delete folder(s)
```

## What it does

1. Resolves the current jj repo (workspace) root.
2. Ensures a sibling container folder `<repo-name>-wts/` exists next to it.
3. Creates a jj workspace inside it at `<repo-name>-wts/<workspace-name>`:
   - `--name` if `-n` is given, otherwise derived from the **parent revision's**
     description (the `-r` revision, or `@-` when `-r` is omitted).
   - The name is lowercased, non-alphanumerics become dashes, capped at 32 chars.
   - Errors if that destination already exists and is non-empty.
   - Passes `-r REV` through to `jj workspace add` so the new working copy sits
     on top of that revision.
4. Copies the untracked files listed in `wts.copy` (see below) from the source
   workspace into the new one.
5. Runs the **action**: by default, `cd`s into the new workspace; or, if you've
   set `wts.action`, runs your script there instead (see below).

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

A leading `~/` in the configured path expands to `$HOME`. The script's stdin,
stdout and stderr are attached to your terminal, so it can be fully interactive.
Its exit code becomes `wts`'s exit code (the workspace is already created either
way). When `wts.action` is set it **replaces** the `cd` — a child process can't
change your shell's directory, so if you want to land in the new workspace, have
your action start a shell there (e.g. `cd "$WTS_DIR"; exec fish`). Unset (the
default) keeps the plain `cd`.

Example `~/.config/wts/on-create.fish` that opens the workspace in your editor
and then drops you into a shell there:

```fish
#!/usr/bin/env fish
$EDITOR $WTS_DIR &
cd $WTS_DIR
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

```fish
cargo install --path ~/Documents/dev/wts          # builds + installs `wts` to ~/.cargo/bin
ln -s ~/Documents/dev/wts/wts.fish ~/.config/fish/conf.d/wts.fish
ln -s ~/Documents/dev/wts/completions/wts.fish ~/.config/fish/completions/wts.fish
```

The fish function shadows the binary and reaches it via `command wts`, so make
sure `~/.cargo/bin` is on `PATH`.

Completions: `wts rm <TAB>` lists the repo's workspaces (with their commit
descriptions, `default` excluded) and `wts -r <TAB>` lists bookmarks.

## Uninstall

Reverse the three install steps — drop the binary and remove the two symlinks:

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
```

`rm` works from the main repo or from inside another workspace, and even from
inside the one you're deleting — when it removes the folder you were standing
in, it prints the main repo path so the shell function cd's you back there. A
name that's neither a known workspace nor a folder on disk is an error.

You can still do it by hand with plain jj:

```
jj workspace list
jj workspace forget <name>
```
