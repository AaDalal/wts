# Completions for `wts`.
#
# Install: symlink into fish's completions dir, e.g.
#     ln -s ~/Documents/dev/wts/completions/wts.fish ~/.config/fish/completions/wts.fish

# Registered jj workspaces (excluding `default`), as name<TAB>commit-description.
function __wts_workspaces
    jj workspace list 2>/dev/null | while read -l line
        set -l parts (string split -m1 ':' -- $line)
        set -l name (string trim -- $parts[1])
        test "$name" = default; and continue
        printf '%s\t%s\n' $name (string trim -- $parts[2])
    end
end

# jj bookmarks, for completing -r/--revision values.
function __wts_bookmarks
    jj bookmark list 2>/dev/null | string replace -rf '^([^:]+):.*' '$1'
end

# No bare file completion for wts.
complete -c wts -f

# Top level (no subcommand yet): the `rm` subcommand + the create flags.
complete -c wts -n __fish_use_subcommand -a rm -d 'Remove workspace(s)'
complete -c wts -n __fish_use_subcommand -s n -l name -r -d 'Name for the new workspace'
complete -c wts -n __fish_use_subcommand -s r -l revision -r -a '(__wts_bookmarks)' \
    -d 'Parent revision for the new workspace'
complete -c wts -s h -l help -d 'Show help'

# After `rm`: complete registered workspace names.
complete -c wts -n '__fish_seen_subcommand_from rm' -a '(__wts_workspaces)'
