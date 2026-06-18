<h1 align="center">wts</h1>

<p align="center">
Create a <a href="https://jj-vcs.github.io/jj/">jujutsu</a> workspace or
<a href="https://git-scm.com/">git</a> worktree and set it up
</p>

<p align="center">
<a href="#how-to-use">How to use</a> &middot;
<a href="#features">Features</a> &middot;
<a href="#actions">Actions</a> &middot;
<a href="#installation">Installation</a>
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/AaDalal/wts/main/demo/demo.gif" alt="wts creating a worktree and opening it in Claude Code" width="80%" />
</p>
<p align="center"><i>wts creates a worktree and opens it in Claude Code</i></p>

#### How to use

1. Run `wts` from your repo to create a new worktree (or jj workspace)
2. wts names it, copies your local files in, and runs your action
3. You land in the new worktree, ready to work
4. Run `wts rm` when you're done to clean it up

wts detects which VCS backs the repo: it uses jj if there's a `.jj` dir at
the repo root (including colocated repos), otherwise git. Both backends
share the same commands and behavior; they differ only in where config lives (jj
config vs git config) and a few git-only knobs (see [Git config](#git-config)).

#### Features

- Creates a jj workspace / git worktree in a sibling folder and opens it, in one command
- Names it from the base revision (change id / commit + description), or pass `-n`
- Runs an [action](#actions) in the new worktree: the built-in `cd`, or your own script (editor, shell, `claude`, tmux, cmux)
- Copies untracked files you list (`.env`, local config) that a fresh worktree doesn't carry over
- `wts rm` removes a worktree (and, on git, its branch), including the one you're in
- Tab completions for worktrees, revisions, and actions — in fish, bash, and zsh

#### Actions

When wts creates a worktree it runs an **action** in it. Actions are named, set
under `wts.action.<name>`. The value is a script path, or the literal `cd` (the
built-in that drops your shell into the worktree).

```fish
# jj backend:
jj config set --user wts.action.default cd                    # bare `wts` cds you in
jj config set --user wts.action.edit ~/.config/wts/edit.fish  # a named action

# git backend (same keys, in git config):
git config --global wts.action.default cd
git config --global wts.action.edit ~/.config/wts/edit.fish
```

```
wts            # runs `default`
wts -a edit    # runs `edit`
wts -a cd      # the built-in, always available
```

`wts` runs `default`; if you haven't set one it errors (pass `-a`, or set a
default). A script action gets the worktree path as `$1`, as `$WTS_DIR`, and as
its working directory, and runs attached to your terminal so it can be
interactive:

```fish
#!/usr/bin/env fish
# ~/.config/wts/edit.fish
$EDITOR $WTS_DIR &
exec fish
```

To open it in cmux instead:

```fish
#!/usr/bin/env fish
cmux new-workspace --focus true --cwd $WTS_DIR --name "(wts) "(basename $WTS_DIR)
```

A script can't cd the shell you ran `wts` from (it's a child process); the
built-in `cd` can. To have a script do it too, write the path to `$WTS_CD_FILE`:
`printf '%s\n' "$WTS_DIR" >$WTS_CD_FILE`.

#### Copying files

A new worktree starts from a clean checkout, so untracked files (`.env`,
`AGENTS.override.md`, local config) don't come along. List globs under `wts.copy`
and wts copies the matches in:

```fish
# jj backend — a table; keys are just labels:
jj config set --user wts.copy.env '.env*'
jj config set --user wts.copy.agents AGENTS.override.md

# git backend — a multi-valued key (use --add to append):
git config --global --add wts.copy '.env*'
git config --global --add wts.copy AGENTS.override.md
```

Entries merge across config layers, so a per-repo (`--repo` / repo-local) entry
*adds to* your user-level ones rather than replacing them. Nothing is copied
unless you opt in.

#### Git config

On the git backend, config lives in git config (INI) and exposes a few extra
knobs for git's branch-oriented worktrees in addition to wts.copy and wts.action.

All are optional:

| Key | Default | What it does |
| --- | --- | --- |
| `wts.baseRef` | `HEAD` | The ref a new worktree is based on when you don't pass `-r`. |
| `wts.createBranch` | `true` | Create a branch named after the worktree (so you have somewhere to commit). `false` checks out a detached HEAD instead. |
| `wts.branchPrefix` | _(empty)_ | Prefix for that branch, e.g. `wts/`, to keep it out of the way of hand-made branches. |
| `wts.containerSuffix` | `-wts` | Suffix for the sibling container dir (`<repo><suffix>`). |

```fish
git config --global wts.createBranch true
git config --global wts.branchPrefix 'wts/'
git config wts.baseRef origin/main      # repo-local: base new worktrees on origin/main
```

By default `wts` creates a branch per worktree (named like the worktree), which
is the git analog of jj's own working-copy commit: a place to accumulate work
that also sidesteps git's refusal to check out the same branch in two worktrees.
`wts rm` deletes that branch along with the worktree.

#### Removing worktrees

```
wts rm <name>...   # remove each one (git: deletes its branch too)
wts rm             # remove the one you're in (and send you back to the main repo)
```

Works from anywhere in the repo. It won't touch the main (`default`) worktree.

### Installation

You'll need [`jj`](https://jj-vcs.github.io/jj/) **or**
[`git`](https://git-scm.com/) (whichever backs your repos), plus
[rust/cargo](https://rust-lang.org/tools/install/).

1. Install the binary:

```fish
cargo install wts
```

2. Load the integration for your shell. `wts init` prints the `wts` function and
   `wts completions` prints the tab completions, for `fish`, `bash`, or `zsh`:

```fish
# fish — in ~/.config/fish/config.fish
wts init fish | source
wts completions fish | source
```

```bash
# bash — in ~/.bashrc
eval "$(wts init bash)"
eval "$(wts completions bash)"
```

```zsh
# zsh — in ~/.zshrc
source <(wts init zsh)
source <(wts completions zsh)
```

The function is needed because a child process can't cd the parent shell: the
binary prints the target directory and the function cds into it.

From a clone, `cargo install --path .` instead of step 1; step 2 is the same.

### Develop

```
cargo build          # debug build at target/debug/wts
cargo test           # unit tests
cargo run -- -n foo  # run without installing (prints the path; no cd)
```
