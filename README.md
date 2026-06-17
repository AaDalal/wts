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
5. `cd`s into the new workspace.

Example: from `~/Documents/dev/sail`, `wts -n hotfix` creates and enters
`~/Documents/dev/sail-wts/hotfix`.

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

The `wts` binary does the work and prints the new workspace path; the fish
function captures that and performs the `cd` (a child process can't change the
parent shell's directory). Requires `jj` on `PATH`.

```fish
cargo install --path ~/Documents/dev/wts          # builds + installs `wts` to ~/.cargo/bin
ln -s ~/Documents/dev/wts/wts.fish ~/.config/fish/conf.d/wts.fish
ln -s ~/Documents/dev/wts/completions/wts.fish ~/.config/fish/completions/wts.fish
```

The fish function shadows the binary and reaches it via `command wts`, so make
sure `~/.cargo/bin` is on `PATH`.

Completions: `wts rm <TAB>` lists the repo's workspaces (with their commit
descriptions, `default` excluded) and `wts -r <TAB>` lists bookmarks.

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
