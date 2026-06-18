# wts.fish: shell wrapper for the wts binary.
#
# The binary embeds this file; load it with `wts init fish | source` in
# ~/.config/fish/config.fish.
#
# A child process can't change the parent shell's cwd, so the `wts` binary
# writes a target directory into the scratch file named by WTS_CD_FILE and this
# function cd's into it: the new workspace on create, or the main repo after
# `wts rm` deletes the folder you were in. Routing cd through a file (rather than
# stdout) keeps the terminal free for a `wts.action` script to run interactively.
# The function shadows the binary; `command wts` reaches the real executable.

function wts --description 'Create or remove a jj workspace in a sibling <repo>-wts/ folder'
    # Let --help/-h print straight to the terminal (clap writes it to stdout).
    if contains -- --help $argv; or contains -- -h $argv
        command wts $argv
        return $status
    end
    # Hand the binary a scratch file to drop a cd-target into; its own stdout,
    # stdin and stderr stay attached to the terminal (so a wts.action script can
    # be interactive). Diagnostics go to stderr.
    set -lx WTS_CD_FILE (mktemp)
    command wts $argv
    set -l st $status
    if test -s $WTS_CD_FILE
        set -l dest (cat $WTS_CD_FILE)
        test -d "$dest"; and cd $dest
    end
    rm -f $WTS_CD_FILE
    return $st
end
