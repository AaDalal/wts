# wts.fish — shell wrapper for the wts binary.
#
# Install the binary first (`cargo install --path .` puts `wts` on PATH), then
# symlink this file into a directory fish autoloads, e.g.
#     ln -s ~/Documents/dev/wts/wts.fish ~/.config/fish/conf.d/wts.fish
#
# A child process can't change the parent shell's cwd, so the `wts` binary
# prints a directory and this function cd's into it: the new workspace on
# create, or the main repo after `wts rm` deletes the folder you were in. The
# function shadows the binary; `command wts` reaches the real executable.

function wts --description 'Create or remove a jj workspace in a sibling <repo>-wts/ folder'
    # Let --help/-h print straight to the terminal (clap writes it to stdout).
    if contains -- --help $argv; or contains -- -h $argv
        command wts $argv
        return $status
    end
    # Binary writes a cd-target path to stdout (if any); diagnostics go to stderr.
    set -l dest (command wts $argv)
    or return $status
    test -n "$dest"; and test -d "$dest"; and cd $dest
    return 0
end
