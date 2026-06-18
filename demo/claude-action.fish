#!/usr/bin/env fish
# wts.action: open the new workspace in Claude Code.
# `wts` runs this in the new workspace dir. Use `exec claude` for an
# interactive session; this demo uses -p so the recording ends cleanly.
claude -p "One short line: confirm you're set up in the "(basename $WTS_DIR)" workspace."
