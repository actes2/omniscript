#!/usr/bin/env bash
# install_editors.sh — Install OmniScript Vim/Neovim & VS Code/Code-OSS editor integrations
# Supports both local installation (symlink or copy) and remote installation over SSH.
#
# Usage:
#   ./install_editors.sh                      # Local install (symlink) for Vim/Neovim + VS Code variants
#   ./install_editors.sh --copy               # Local install by copying files instead of symlinking
#   ./install_editors.sh --vim-only           # Only install Vim / Neovim plugin
#   ./install_editors.sh --vscode-only        # Only install VS Code / Code-OSS / VSCodium / Cursor extension
#   ./install_editors.sh --ssh user@host      # Push and install both plugins onto a remote host via SSH
#   ./install_editors.sh --ssh user@host -p 2222
#   ./install_editors.sh --uninstall          # Remove installed plugins (local or with --ssh user@host)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VIM_SRC="$SCRIPT_DIR/editors/vim-omniscript"
VSCODE_SRC="$SCRIPT_DIR/editors/vscode-omniscript"

INSTALL_VIM=1
INSTALL_VSCODE=1
MODE="symlink"      # symlink | copy
UNINSTALL=0
SSH_TARGET=""
SSH_PORT=""

usage() {
    cat <<'EOF'
Usage: ./install_editors.sh [OPTIONS]

Install OmniScript editor plugins for Vim/Neovim and VS Code / Code-OSS / VSCodium / Cursor.

Options:
  --vim-only           Install (or uninstall) only the Vim / Neovim plugin
  --vscode-only        Install (or uninstall) only the VS Code / Code-OSS extension
  --copy               Copy plugin files instead of symlinking (local mode; SSH always copies)
  --symlink            Symlink plugin directories from this repo (default for local mode)
  --ssh <user@host>    Install (or uninstall) on a remote host over SSH (includes ~/.vscode-server)
  -p, --port <port>    SSH port (used with --ssh)
  --uninstall          Remove installed OmniScript editor plugins
  -h, --help           Show this help message
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --vim-only)
            INSTALL_VIM=1
            INSTALL_VSCODE=0
            shift
            ;;
        --vscode-only)
            INSTALL_VIM=0
            INSTALL_VSCODE=1
            shift
            ;;
        --copy)
            MODE="copy"
            shift
            ;;
        --symlink)
            MODE="symlink"
            shift
            ;;
        --ssh)
            SSH_TARGET="${2:-}"
            [[ -n "$SSH_TARGET" ]] || { echo "Error: --ssh requires a target (e.g. user@host)" >&2; exit 1; }
            shift 2
            ;;
        -p|--port)
            SSH_PORT="${2:-}"
            [[ -n "$SSH_PORT" ]] || { echo "Error: --port requires a port number" >&2; exit 1; }
            shift 2
            ;;
        --uninstall)
            UNINSTALL=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

# Parse extension version from editors/vscode-omniscript/package.json
EXT_VERSION="0.2.2"
if [[ -f "$VSCODE_SRC/package.json" ]]; then
    parsed_ver="$(grep -m1 '"version"' "$VSCODE_SRC/package.json" | sed -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/' || true)"
    if [[ -n "$parsed_ver" ]]; then
        EXT_VERSION="$parsed_ver"
    fi
fi
VSCODE_EXT_DIRNAME="omniscript-${EXT_VERSION}"

# Verify source directories exist when installing
if [[ "$UNINSTALL" -eq 0 ]]; then
    if [[ "$INSTALL_VIM" -eq 1 && ! -d "$VIM_SRC" ]]; then
        echo "Error: Vim plugin source not found at $VIM_SRC" >&2
        exit 1
    fi
    if [[ "$INSTALL_VSCODE" -eq 1 && ! -d "$VSCODE_SRC" ]]; then
        echo "Error: VS Code extension source not found at $VSCODE_SRC" >&2
        exit 1
    fi
fi

# ---------------------------------------------------------------------------
# Local install / uninstall helpers
# ---------------------------------------------------------------------------
install_dir_local() {
    local src="$1"
    local dst="$2"
    mkdir -p "$(dirname "$dst")"
    rm -rf "$dst"
    if [[ "$MODE" == "symlink" ]]; then
        ln -s "$src" "$dst"
        echo "  [symlink] $dst -> $src"
    else
        cp -a "$src" "$dst"
        echo "  [copy]    $dst"
    fi
}

run_local() {
    if [[ "$UNINSTALL" -eq 1 ]]; then
        echo "==> Uninstalling OmniScript editor plugins locally..."
        if [[ "$INSTALL_VIM" -eq 1 ]]; then
            for d in \
                "$HOME/.vim/pack/plugins/start/vim-omniscript" \
                "$HOME/.local/share/nvim/site/pack/plugins/start/vim-omniscript"; do
                if [[ -e "$d" || -L "$d" ]]; then
                    rm -rf "$d"
                    echo "  [removed] $d"
                fi
            done
        fi
        if [[ "$INSTALL_VSCODE" -eq 1 ]]; then
            for base in \
                "$HOME/.vscode/extensions" \
                "$HOME/.vscode-oss/extensions" \
                "$HOME/.vscode-insiders/extensions" \
                "$HOME/.vscode-server/extensions" \
                "$HOME/.cursor/extensions" \
                "$HOME/.vscodium/extensions"; do
                if [[ -d "$base" ]]; then
                    for d in "$base"/omniscript-*; do
                        if [[ -e "$d" || -L "$d" ]]; then
                            rm -rf "$d"
                            echo "  [removed] $d"
                        fi
                    done
                fi
            done
        fi
        echo "Done."
        return
    fi

    if [[ "$INSTALL_VIM" -eq 1 ]]; then
        echo "==> Installing Vim / Neovim plugin ($MODE)..."
        install_dir_local "$VIM_SRC" "$HOME/.vim/pack/plugins/start/vim-omniscript"
        if command -v nvim >/dev/null 2>&1 || [[ -d "$HOME/.local/share/nvim" || -d "$HOME/.config/nvim" ]]; then
            install_dir_local "$VIM_SRC" "$HOME/.local/share/nvim/site/pack/plugins/start/vim-omniscript"
        fi
    fi

    if [[ "$INSTALL_VSCODE" -eq 1 ]]; then
        echo "==> Installing VS Code / Code-OSS extension ($VSCODE_EXT_DIRNAME, $MODE)..."
        local installed_any=0
        for base in \
            "$HOME/.vscode/extensions" \
            "$HOME/.vscode-oss/extensions" \
            "$HOME/.vscode-insiders/extensions" \
            "$HOME/.vscode-server/extensions" \
            "$HOME/.cursor/extensions" \
            "$HOME/.vscodium/extensions"; do
            if [[ -d "$base" || -d "$(dirname "$base")" ]]; then
                # Remove older omniscript-* versions in this extensions dir
                for old in "$base"/omniscript-*; do
                    [[ -e "$old" || -L "$old" ]] && rm -rf "$old"
                done
                install_dir_local "$VSCODE_SRC" "$base/$VSCODE_EXT_DIRNAME"
                installed_any=1
            fi
        done
        if [[ "$installed_any" -eq 0 ]]; then
            # Fallback default if no VS Code config dir exists yet
            install_dir_local "$VSCODE_SRC" "$HOME/.vscode/extensions/$VSCODE_EXT_DIRNAME"
        fi
        echo "  Tip: Reload VS Code / Code-OSS (Ctrl+Shift+P -> Developer: Reload Window)."
    fi
    echo "Done."
}

# ---------------------------------------------------------------------------
# Remote SSH install / uninstall
# ---------------------------------------------------------------------------
run_ssh() {
    local ssh_opts=()
    if [[ -n "$SSH_PORT" ]]; then
        ssh_opts+=(-p "$SSH_PORT")
    fi

    if [[ "$UNINSTALL" -eq 1 ]]; then
        echo "==> Uninstalling OmniScript editor plugins on $SSH_TARGET..."
        ssh "${ssh_opts[@]}" "$SSH_TARGET" bash -s -- "$INSTALL_VIM" "$INSTALL_VSCODE" <<'REMOTE_UNINSTALL'
set -euo pipefail
INSTALL_VIM="$1"
INSTALL_VSCODE="$2"

if [[ "$INSTALL_VIM" -eq 1 ]]; then
    for d in \
        "$HOME/.vim/pack/plugins/start/vim-omniscript" \
        "$HOME/.local/share/nvim/site/pack/plugins/start/vim-omniscript"; do
        if [[ -e "$d" || -L "$d" ]]; then
            rm -rf "$d"
            echo "  [remote removed] $d"
        fi
    done
fi

if [[ "$INSTALL_VSCODE" -eq 1 ]]; then
    for base in \
        "$HOME/.vscode/extensions" \
        "$HOME/.vscode-oss/extensions" \
        "$HOME/.vscode-insiders/extensions" \
        "$HOME/.vscode-server/extensions" \
        "$HOME/.cursor/extensions" \
        "$HOME/.vscodium/extensions"; do
        if [[ -d "$base" ]]; then
            for d in "$base"/omniscript-*; do
                if [[ -e "$d" || -L "$d" ]]; then
                    rm -rf "$d"
                    echo "  [remote removed] $d"
                fi
            done
        fi
    done
fi
REMOTE_UNINSTALL
        echo "Done."
        return
    fi

    echo "==> Deploying OmniScript editor plugins to $SSH_TARGET via SSH..."
    local tar_paths=()
    [[ "$INSTALL_VIM" -eq 1 ]] && tar_paths+=("vim-omniscript")
    [[ "$INSTALL_VSCODE" -eq 1 ]] && tar_paths+=("vscode-omniscript")

    local remote_script
    remote_script="$(cat <<'REMOTE_INSTALL'
set -euo pipefail
INSTALL_VIM="$1"
INSTALL_VSCODE="$2"
VSCODE_EXT_DIRNAME="$3"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
tar -C "$TMP_DIR" -xzf -

if [ "$INSTALL_VIM" -eq 1 ] && [ -d "$TMP_DIR/vim-omniscript" ]; then
    mkdir -p "$HOME/.vim/pack/plugins/start"
    rm -rf "$HOME/.vim/pack/plugins/start/vim-omniscript"
    cp -a "$TMP_DIR/vim-omniscript" "$HOME/.vim/pack/plugins/start/vim-omniscript"
    echo "  [remote installed] ~/.vim/pack/plugins/start/vim-omniscript"

    if command -v nvim >/dev/null 2>&1 || [ -d "$HOME/.local/share/nvim" ] || [ -d "$HOME/.config/nvim" ]; then
        mkdir -p "$HOME/.local/share/nvim/site/pack/plugins/start"
        rm -rf "$HOME/.local/share/nvim/site/pack/plugins/start/vim-omniscript"
        cp -a "$TMP_DIR/vim-omniscript" "$HOME/.local/share/nvim/site/pack/plugins/start/vim-omniscript"
        echo "  [remote installed] ~/.local/share/nvim/site/pack/plugins/start/vim-omniscript"
    fi
fi

if [ "$INSTALL_VSCODE" -eq 1 ] && [ -d "$TMP_DIR/vscode-omniscript" ]; then
    installed_any=0
    for base in \
        "$HOME/.vscode-server/extensions" \
        "$HOME/.vscode/extensions" \
        "$HOME/.vscode-oss/extensions" \
        "$HOME/.vscode-insiders/extensions" \
        "$HOME/.cursor/extensions" \
        "$HOME/.vscodium/extensions"; do
        if [ -d "$base" ] || [ -d "$(dirname "$base")" ]; then
            mkdir -p "$base"
            for old in "$base"/omniscript-*; do
                [ -e "$old" ] || [ -L "$old" ] && rm -rf "$old"
            done
            cp -a "$TMP_DIR/vscode-omniscript" "$base/$VSCODE_EXT_DIRNAME"
            echo "  [remote installed] $base/$VSCODE_EXT_DIRNAME"
            installed_any=1
        fi
    done
    if [ "$installed_any" -eq 0 ]; then
        mkdir -p "$HOME/.vscode-server/extensions"
        cp -a "$TMP_DIR/vscode-omniscript" "$HOME/.vscode-server/extensions/$VSCODE_EXT_DIRNAME"
        echo "  [remote installed] ~/.vscode-server/extensions/$VSCODE_EXT_DIRNAME"
    fi
fi
REMOTE_INSTALL
)"
    local remote_b64
    remote_b64="$(printf '%s' "$remote_script" | base64 | tr -d '\n')"

    tar -C "$SCRIPT_DIR/editors" -czf - "${tar_paths[@]}" | \
        ssh "${ssh_opts[@]}" "$SSH_TARGET" \
            "bash -c \"\$(printf '%s' '$remote_b64' | base64 -d)\" _ '$INSTALL_VIM' '$INSTALL_VSCODE' '$VSCODE_EXT_DIRNAME'"
    echo "Done. Reload VS Code / Remote-SSH window to activate."
}

if [[ -n "$SSH_TARGET" ]]; then
    run_ssh
else
    run_local
fi
