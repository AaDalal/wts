# wts.bash: shell wrapper for the wts binary.
#
# The binary embeds this file; load it with `wts init bash` in ~/.bashrc:
#     eval "$(command wts init bash)"
#
# A child process can't change the parent shell's cwd, so the `wts` binary
# writes a target directory into the scratch file named by WTS_CD_FILE and this
# function cd's into it: the new workspace on create, or the main repo after
# `wts rm` deletes the folder you were in. Routing cd through a file (rather than
# stdout) keeps the terminal free for a `wts.action` script to run interactively.
# The function shadows the binary; `command wts` reaches the real executable.

wts() {
    # Let --help/-h print straight to the terminal (clap writes it to stdout).
    local arg
    for arg in "$@"; do
        if [[ "$arg" == --help || "$arg" == -h ]]; then
            command wts "$@"
            return $?
        fi
    done
    # Hand the binary a scratch file to drop a cd-target into; its own stdout,
    # stdin and stderr stay attached to the terminal (so a wts.action script can
    # be interactive). Diagnostics go to stderr.
    local WTS_CD_FILE
    WTS_CD_FILE=$(mktemp)
    export WTS_CD_FILE
    command wts "$@"
    local st=$?
    if [[ -s "$WTS_CD_FILE" ]]; then
        local dest
        dest=$(cat "$WTS_CD_FILE")
        [[ -d "$dest" ]] && cd "$dest"
    fi
    rm -f "$WTS_CD_FILE"
    return $st
}
