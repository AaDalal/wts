# wts build/install helpers. Run `just` to list recipes.

repo := justfile_directory()
fish_conf := env_var('HOME') / ".config/fish/conf.d/wts.fish"
fish_comp := env_var('HOME') / ".config/fish/completions/wts.fish"

# List available recipes.
default:
    @just --list

# Build the binary (debug).
build:
    cargo build

# Run the test suite.
test:
    cargo test

# Install the binary, then symlink the fish function and completions.
install:
    cargo install --path "{{repo}}"
    mkdir -p "{{parent_directory(fish_conf)}}" "{{parent_directory(fish_comp)}}"
    ln -sf "{{repo}}/wts.fish" "{{fish_conf}}"
    ln -sf "{{repo}}/completions/wts.fish" "{{fish_comp}}"

# Remove the binary and the fish symlinks (no-op if already gone).
uninstall:
    -cargo uninstall wts
    rm -f "{{fish_conf}}" "{{fish_comp}}"

# Uninstall, then install.
reinstall: uninstall install
