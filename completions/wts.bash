# Completions for `wts` (bash).
#
# The binary embeds this file; load it with `wts completions bash` in ~/.bashrc:
#     eval "$(command wts completions bash)"

# Detects the active VCS backend, matching the binary: jj if a `.jj` dir exists
# at the workspace root, else git, else none.
__wts_backend() {
    local root
    root=$(jj workspace root 2>/dev/null)
    if [[ -n "$root" && -d "$root/.jj" ]]; then echo jj; return; fi
    if git rev-parse --show-toplevel >/dev/null 2>&1; then echo git; return; fi
    echo none
}

# Bookmarks (jj) or local branches (git), for completing -r/--revision values.
__wts_bookmarks() {
    case "$(__wts_backend)" in
        jj)
            jj bookmark list 2>/dev/null | sed -n 's/^\([^:]*\):.*/\1/p'
            ;;
        git)
            git for-each-ref --format='%(refname:short)' refs/heads 2>/dev/null
            ;;
    esac
}

# Configured action names (`wts.action.<name>`) plus the built-in `cd`, for
# completing -a/--action.
__wts_actions() {
    printf '%s\n' cd
    case "$(__wts_backend)" in
        jj)
            jj config list wts.action 2>/dev/null \
                | sed -n 's/^wts\.action\.\([^ =]*\)[[:space:]]*=.*/\1/p'
            ;;
        git)
            git config --get-regexp '^wts\.action\.' 2>/dev/null \
                | sed -n 's/^wts\.action\.\([^ ]*\) .*/\1/p'
            ;;
    esac
}

# Registered workspace/worktree names (excluding the main/default one), for
# completing `rm`.
__wts_workspaces() {
    case "$(__wts_backend)" in
        jj)
            jj workspace list 2>/dev/null | sed -n 's/^\([^:]*\):.*/\1/p' | grep -vx default
            ;;
        git)
            git worktree list --porcelain 2>/dev/null \
                | awk '/^worktree /{n++; if(n>1){sub(/^worktree /,""); print}}' \
                | while read -r p; do basename "$p"; done
            ;;
    esac
}

_wts() {
    local cur prev words cword
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    # Value completion for flags that take an argument.
    case "$prev" in
        -r|--revision)
            COMPREPLY=( $(compgen -W "$(__wts_bookmarks)" -- "$cur") )
            return
            ;;
        -a|--action)
            COMPREPLY=( $(compgen -W "$(__wts_actions)" -- "$cur") )
            return
            ;;
        -n|--name)
            # Free-form; no completions.
            COMPREPLY=()
            return
            ;;
    esac

    # Find the first non-flag word after `wts` to see which subcommand we're in.
    local i subcmd=""
    for (( i=1; i < COMP_CWORD; i++ )); do
        case "${COMP_WORDS[i]}" in
            -*) ;;
            *) subcmd="${COMP_WORDS[i]}"; break ;;
        esac
    done

    case "$subcmd" in
        rm)
            # Complete registered workspace names.
            COMPREPLY=( $(compgen -W "$(__wts_workspaces)" -- "$cur") )
            return
            ;;
        init|completions)
            # Each takes a shell name.
            COMPREPLY=( $(compgen -W "fish bash zsh" -- "$cur") )
            return
            ;;
    esac

    # Top level (no subcommand yet): the subcommands + the create flags.
    COMPREPLY=( $(compgen -W "rm init completions -n --name -r --revision -a --action -h --help" -- "$cur") )
}

complete -F _wts wts
