# Completions for `wts`.
#
# The binary embeds this file; load it with `wts completions fish | source` in
# ~/.config/fish/config.fish.

# Detect the active VCS backend, matching the binary: jj if a `.jj` dir exists at
# the repo root, else git, else none.
function __wts_backend
    set -l root (jj workspace root 2>/dev/null)
    if test -n "$root"; and test -d "$root/.jj"
        echo jj; return
    end
    if git rev-parse --show-toplevel >/dev/null 2>&1
        echo git; return
    end
    echo none
end

# Registered workspaces (excluding the main/default), as name<TAB>description.
function __wts_workspaces
    switch (__wts_backend)
        case jj
            jj workspace list 2>/dev/null | while read -l line
                set -l parts (string split -m1 ':' -- $line)
                set -l name (string trim -- $parts[1])
                test "$name" = default; and continue
                printf '%s\t%s\n' $name (string trim -- $parts[2])
            end
        case git
            # Linked worktree folder names; skip the first record (main worktree).
            set -l paths (git worktree list --porcelain 2>/dev/null | string match -rg '^worktree (.+)$')
            for p in $paths[2..-1]
                printf '%s\n' (basename $p)
            end
    end
end

# Bookmarks/branches, for completing -r/--revision values.
function __wts_bookmarks
    switch (__wts_backend)
        case jj
            jj bookmark list 2>/dev/null | string replace -rf '^([^:]+):.*' '$1'
        case git
            git for-each-ref --format='%(refname:short)' refs/heads 2>/dev/null
    end
end

# Configured action names (`wts.action.<name>`) plus the built-in `cd`, as
# name<TAB>value, for completing -a/--action.
function __wts_actions
    printf '%s\t%s\n' cd 'built-in: cd into the workspace'
    switch (__wts_backend)
        case jj
            jj config list wts.action 2>/dev/null | while read -l line
                set -l m (string match -rg '^wts\.action\.([^ =]+)\s*=\s*(.*)$' -- $line)
                test (count $m) -ge 2; and printf '%s\t%s\n' $m[1] (string trim -c '"' -- $m[2])
            end
        case git
            git config --get-regexp '^wts\.action\.' 2>/dev/null | while read -l line
                set -l m (string match -rg '^wts\.action\.(\S+) (.*)$' -- $line)
                test (count $m) -ge 2; and printf '%s\t%s\n' $m[1] $m[2]
            end
    end
end

# No bare file completion for wts.
complete -c wts -f

# Top level (no subcommand yet): subcommands + the create flags.
# `-x` (exclusive) = requires an argument AND suppresses file completion for it;
# plain `-r`/`--require-parameter` would still let fish offer filenames.
complete -c wts -n __fish_use_subcommand -a rm -d 'Remove workspace(s)'
complete -c wts -n __fish_use_subcommand -a init -d 'Print shell init snippet'
complete -c wts -n __fish_use_subcommand -a completions -d 'Print shell completions'
complete -c wts -n __fish_use_subcommand -s n -l name -x -d 'Name for the new workspace'
complete -c wts -n __fish_use_subcommand -s r -l revision -x -a '(__wts_bookmarks)' \
    -d 'Parent revision for the new workspace'
complete -c wts -n __fish_use_subcommand -s a -l action -x -a '(__wts_actions)' \
    -d 'Action to run in the new workspace'
complete -c wts -s h -l help -d 'Show help'

# After `rm`: complete registered workspace names.
complete -c wts -n '__fish_seen_subcommand_from rm' -a '(__wts_workspaces)'

# After `init`/`completions`: complete the shell argument.
complete -c wts -n '__fish_seen_subcommand_from init completions' \
    -a 'fish bash zsh' -d 'Shell'
