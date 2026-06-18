#!/usr/bin/env fish
# Drives the asciinema demo: shows the wts.action that opens Claude Code, then
# runs `wts` to create a workspace and hand it off. Pacing via sleeps so the
# recording is watchable. Expects $WTS_DEMO_DIR (sandbox repo) and $JJ_CONFIG.

cd $WTS_DEMO_DIR

function _say   # narration line, dimmed
    printf '\n\033[2m# %s\033[0m\n' "$argv"
    sleep 0.9
end

function _run   # echo a prompt + command, then run it
    printf '\n\033[1;36m❯\033[0m %s\n' "$argv"
    sleep 0.7
    eval $argv
    sleep 1.0
end

sleep 0.6
_say "wts creates a jj workspace in a sibling folder and runs an action in it."
_say "Here the action opens the new workspace in Claude Code:"
_run 'cat ~/.config/wts/claude.fish'

_say "Create a workspace for some new work — wts hands it straight to Claude:"
_run 'wts -n add-dark-mode'

_say "Claude ran inside .../acme-api-wts/add-dark-mode — a fresh, isolated workspace."
sleep 1.2
