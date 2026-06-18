#compdef wts
# Completions for `wts`.
#
# The binary embeds this file; load it with `wts completions zsh | source` in
# ~/.zshrc. Sourcing it defines `_wts` and registers it via the `compdef` call
# at the end, so it works without being dropped into $fpath.

# Detect the VCS backend wts would use here: jj if a `.jj` dir exists at the
# workspace root, else git, else none. Mirrors the binary's own selection.
__wts_backend() {
    local root
    root=$(jj workspace root 2>/dev/null)
    if [[ -n $root && -d $root/.jj ]]; then echo jj; return; fi
    if git rev-parse --show-toplevel >/dev/null 2>&1; then echo git; return; fi
    echo none
}

# Workspaces other than the main/default one, as name:description.
# jj: `jj workspace list` minus `default`. git: linked worktree basenames.
__wts_workspaces() {
    local -a ws
    local line name desc wt branch
    case $(__wts_backend) in
        (jj)
            jj workspace list 2>/dev/null | while IFS= read -r line; do
                name=${line%%:*}
                [[ $name == default ]] && continue
                desc=${line#*:}
                # Trim a single leading space after the colon.
                desc=${desc# }
                ws+=("${name}:${desc}")
            done
            ;;
        (git)
            # `git worktree list --porcelain` emits a `worktree <path>` line per
            # worktree, the first being the main one; skip it. A `branch
            # refs/heads/<x>` line for the same record makes a nice description.
            local first=1
            git worktree list --porcelain 2>/dev/null | while IFS= read -r line; do
                if [[ $line == worktree\ * ]]; then
                    wt=${line#worktree }
                    if (( first )); then first=0; continue; fi
                    ws+=("${wt:t}:${wt}")
                elif [[ $line == branch\ refs/heads/* ]]; then
                    # Replace the just-added entry's description with the branch.
                    branch=${line#branch refs/heads/}
                    [[ ${#ws} -gt 0 ]] && ws[-1]="${wt:t}:${branch}"
                fi
            done
            ;;
    esac
    _describe -t workspaces 'workspace' ws
}

# Revision/branch names for completing -r/--revision.
# jj: bookmarks. git: local branch names.
__wts_bookmarks() {
    local -a bms
    local line
    case $(__wts_backend) in
        (jj)
            jj bookmark list 2>/dev/null | while IFS= read -r line; do
                [[ $line == *:* ]] || continue
                bms+=("${line%%:*}")
            done
            ;;
        (git)
            git for-each-ref --format='%(refname:short)' refs/heads 2>/dev/null | while IFS= read -r line; do
                [[ -n $line ]] && bms+=("$line")
            done
            ;;
    esac
    _describe -t bookmarks 'bookmark' bms
}

# Configured action names (`wts.action.<name>`) plus the built-in `cd`, as
# name:value, for completing -a/--action.
__wts_actions() {
    local -a acts
    local line key val
    acts=('cd:built-in: cd into the workspace')
    case $(__wts_backend) in
        (jj)
            jj config list wts.action 2>/dev/null | while IFS= read -r line; do
                # Lines look like: wts.action.<name> = "<value>"
                if [[ $line =~ '^wts\.action\.([^ =]+)[[:space:]]*=[[:space:]]*(.*)$' ]]; then
                    key=${match[1]}
                    val=${match[2]}
                    # Strip surrounding double quotes from the TOML value.
                    val=${val#\"}
                    val=${val%\"}
                    acts+=("${key}:${val}")
                fi
            done
            ;;
        (git)
            # Lines look like: wts.action.<name> <value> (single space, key has
            # no spaces).
            git config --get-regexp '^wts\.action\.' 2>/dev/null | while IFS= read -r line; do
                key=${line%% *}
                val=${line#* }
                key=${key#wts.action.}
                acts+=("${key}:${val}")
            done
            ;;
    esac
    _describe -t actions 'action' acts
}

_wts() {
    local curcontext="$curcontext" state line
    typeset -A opt_args

    # Create flags are valid at the top level (no subcommand). `->subcmd` lets us
    # branch on the chosen subcommand below.
    _arguments -C \
        '(- *)'{-h,--help}'[Show help]' \
        '(-n --name)'{-n,--name}'[Name for the new workspace]:name:' \
        '(-r --revision)'{-r,--revision}'[Parent revision for the new workspace]:revision:__wts_bookmarks' \
        '(-a --action)'{-a,--action}'[Action to run in the new workspace]:action:__wts_actions' \
        '1: :->subcmd' \
        '*:: :->args' \
        && return 0

    case $state in
        subcmd)
            local -a subcmds
            subcmds=(
                'rm:Remove workspace(s)'
                'init:Print the shell integration'
                'completions:Print shell completions'
            )
            _describe -t commands 'wts command' subcmds
            ;;
        args)
            case $line[1] in
                rm)
                    __wts_workspaces
                    ;;
                init|completions)
                    local -a shells
                    shells=('fish:fish shell' 'bash:bash shell' 'zsh:zsh shell')
                    _describe -t shells 'shell' shells
                    ;;
            esac
            ;;
    esac
}

# Register the completion. `compdef` exists once `compinit` has run (the normal
# case in an interactive shell); initialize it ourselves if sourced before that.
if (( ! ${+functions[compdef]} )); then
    autoload -Uz compinit && compinit -u
fi
compdef _wts wts
