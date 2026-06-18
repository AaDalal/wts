<h1 align="center">wts</h1>

<p align="center">
Spin up a <a href="https://jj-vcs.github.io/jj/">jujutsu</a> workspace in a sibling folder and jump straight into it
</p>

<p align="center">
<a href="#how-to-use">How to use</a> &middot;
<a href="#features">Features</a> &middot;
<a href="#actions">Actions</a> &middot;
<a href="#installation">Installation</a>
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/AaDalal/wts/main/demo/demo.gif" alt="wts creating a workspace and opening it in Claude Code" width="80%" />
</p>
<p align="center"><i>wts creates a workspace and opens it in Claude Code</i></p>

#### How to use

1. Run `wts` from your repo to create a new workspace
2. wts names it, copies your local files in, and runs your action
3. You land in the new workspace, ready to work
4. Run `wts rm` when you're done to clean it up

Each workspace is a real jj workspace in a sibling `<repo>-wts/<name>` folder, so
you can work on several things at once without stashing or re-cloning.

#### Features

- Creates a jj workspace in a sibling folder and opens it, in one command
- Names it from the base revision (change id + description), or pass `-n`
- Runs an [action](#actions) in the new workspace: the built-in `cd`, or your own script (editor, shell, `claude`, tmux, cmux)
- Copies untracked files you list (`.env`, local config) that jj leaves behind
- `wts rm` forgets and deletes a workspace, including the one you're in
- Tab completions for workspaces, revisions, and actions

#### Actions

When wts creates a workspace it runs an **action** in it. Actions are named, set
under `wts.action.<name>`. The value is a script path, or the literal `cd` (the
built-in that drops your shell into the workspace).

```fish
jj config set --user wts.action.default cd                    # bare `wts` cds you in
jj config set --user wts.action.edit ~/.config/wts/edit.fish  # a named action
```

```
wts            # runs `default`
wts -a edit    # runs `edit`
wts -a cd      # the built-in, always available
```

`wts` runs `default`; if you haven't set one it errors (pass `-a`, or set a
default). A script action gets the workspace path as `$1`, as `$WTS_DIR`, and as
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

jj brings your tracked files into a new workspace but leaves untracked ones
(`.env`, `AGENTS.override.md`, local config) behind. List globs under `wts.copy`
and wts copies the matches in:

```fish
jj config set --user wts.copy.env '.env*'
jj config set --user wts.copy.agents AGENTS.override.md
```

It's a table, so `--repo` entries add to your `--user` ones. Nothing is copied
unless you opt in.

#### Removing workspaces

```
wts rm <name>...   # forget + delete each one
wts rm             # remove the one you're in (and send you back to the main repo)
```

Works from anywhere in the repo. It won't touch the main (`default`) workspace.

### Installation

You'll need [`jj`](https://jj-vcs.github.io/jj/) and
[rust/cargo](https://rust-lang.org/tools/install/).

1. Install the binary:

```fish
cargo install wts
```

2. Load the fish integration in `~/.config/fish/config.fish`:

```fish
wts init fish | source          # the `wts` function (does the cd)
wts completions fish | source   # tab completions
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
