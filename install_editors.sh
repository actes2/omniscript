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

# Parse extension metadata from editors/vscode-omniscript/package.json
EXT_PUBLISHER="omniscript"
EXT_NAME="omniscript"
EXT_VERSION="0.4.0"
if [[ -f "$VSCODE_SRC/package.json" ]]; then
    parsed_pub="$(grep -m1 '"publisher"' "$VSCODE_SRC/package.json" | sed -E 's/.*"publisher"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/' || true)"
    parsed_name="$(grep -m1 '"name"' "$VSCODE_SRC/package.json" | sed -E 's/.*"name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/' || true)"
    parsed_ver="$(grep -m1 '"version"' "$VSCODE_SRC/package.json" | sed -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/' || true)"
    [[ -n "$parsed_pub" ]] && EXT_PUBLISHER="$parsed_pub"
    [[ -n "$parsed_name" ]] && EXT_NAME="$parsed_name"
    [[ -n "$parsed_ver" ]] && EXT_VERSION="$parsed_ver"
fi
EXT_ID="${EXT_PUBLISHER}.${EXT_NAME}"
VSCODE_EXT_DIRNAME="${EXT_ID}-${EXT_VERSION}"

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

# Sync modern VS Code (>= 1.74) profile registry (extensions.json + .obsolete)
sync_vscode_registry() {
    local action="$1"   # register | unregister
    local base="$2"
    if command -v python3 >/dev/null 2>&1; then
        python3 - "$action" "$base" "$EXT_ID" "$EXT_VERSION" "$VSCODE_EXT_DIRNAME" <<'PYEOF'
import json, os, sys, time

action, base, ext_id, ext_ver, dirname = sys.argv[1:6]
ext_id_lower = ext_id.lower()
obsolete_path = os.path.join(base, ".obsolete")
registry_path = os.path.join(base, "extensions.json")

if os.path.isfile(obsolete_path):
    try:
        with open(obsolete_path, "r", encoding="utf-8") as f:
            obs = json.load(f)
        if isinstance(obs, dict):
            cleaned = {
                k: v for k, v in obs.items()
                if not (k.lower().startswith(ext_id_lower + "-") or k.lower().startswith("omniscript-"))
            }
            if cleaned != obs:
                tmp = obsolete_path + ".tmp"
                with open(tmp, "w", encoding="utf-8") as f:
                    json.dump(cleaned, f)
                os.replace(tmp, obsolete_path)
    except Exception as exc:
        print(f"  [warn] could not update {obsolete_path}: {exc}", file=sys.stderr)

if os.path.isfile(registry_path):
    try:
        with open(registry_path, "r", encoding="utf-8") as f:
            entries = json.load(f)
        if isinstance(entries, list):
            filtered = [
                e for e in entries
                if not (
                    isinstance(e, dict)
                    and str(e.get("identifier", {}).get("id", "")).lower() == ext_id_lower
                )
            ]
            if action == "register":
                filtered.append({
                    "identifier": {"id": ext_id},
                    "version": ext_ver,
                    "location": {
                        "$mid": 1,
                        "path": os.path.join(base, dirname),
                        "scheme": "file",
                    },
                    "relativeLocation": dirname,
                    "metadata": {
                        "installedTimestamp": int(time.time() * 1000),
                        "pinned": True,
                        "source": "vsix",
                    },
                })
            if filtered != entries:
                tmp = registry_path + ".tmp"
                with open(tmp, "w", encoding="utf-8") as f:
                    json.dump(filtered, f, separators=(",", ":"))
                os.replace(tmp, registry_path)
                print(f"  [registry] {action}ed {ext_id}@{ext_ver} in {registry_path}")
    except Exception as exc:
        print(f"  [warn] could not update {registry_path}: {exc}", file=sys.stderr)
PYEOF
    elif command -v node >/dev/null 2>&1; then
        node - "$action" "$base" "$EXT_ID" "$EXT_VERSION" "$VSCODE_EXT_DIRNAME" <<'NODEEOF'
const fs = require("fs");
const path = require("path");
const [action, base, extId, extVer, dirname] = process.argv.slice(2);
const extIdLower = extId.toLowerCase();
const obsoletePath = path.join(base, ".obsolete");
const registryPath = path.join(base, "extensions.json");

if (fs.existsSync(obsoletePath)) {
  try {
    const obs = JSON.parse(fs.readFileSync(obsoletePath, "utf8"));
    let changed = false;
    for (const k of Object.keys(obs)) {
      if (k.toLowerCase().startsWith(extIdLower + "-") || k.toLowerCase().startsWith("omniscript-")) {
        delete obs[k];
        changed = true;
      }
    }
    if (changed) fs.writeFileSync(obsoletePath, JSON.stringify(obs));
  } catch (_) {}
}

if (fs.existsSync(registryPath)) {
  try {
    const entries = JSON.parse(fs.readFileSync(registryPath, "utf8"));
    if (Array.isArray(entries)) {
      const filtered = entries.filter(
        (e) => !(e && e.identifier && String(e.identifier.id || "").toLowerCase() === extIdLower)
      );
      if (action === "register") {
        filtered.push({
          identifier: { id: extId },
          version: extVer,
          location: { $mid: 1, path: path.join(base, dirname), scheme: "file" },
          relativeLocation: dirname,
          metadata: { installedTimestamp: Date.now(), pinned: true, source: "vsix" },
        });
      }
      fs.writeFileSync(registryPath, JSON.stringify(filtered));
      console.log(`  [registry] ${action}ed ${extId}@${extVer} in ${registryPath}`);
    }
  } catch (_) {}
}
NODEEOF
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
                    for d in "$base"/omniscript-* "$base"/"${EXT_ID}"-*; do
                        if [[ -e "$d" || -L "$d" ]]; then
                            rm -rf "$d"
                            echo "  [removed] $d"
                        fi
                    done
                    sync_vscode_registry "unregister" "$base"
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
                # Remove older omniscript-* and publisher.name-* versions in this extensions dir
                for old in "$base"/omniscript-* "$base"/"${EXT_ID}"-*; do
                    [[ -e "$old" || -L "$old" ]] && rm -rf "$old"
                done
                install_dir_local "$VSCODE_SRC" "$base/$VSCODE_EXT_DIRNAME"
                sync_vscode_registry "register" "$base"
                installed_any=1
            fi
        done
        if [[ "$installed_any" -eq 0 ]]; then
            # Fallback default if no VS Code config dir exists yet
            install_dir_local "$VSCODE_SRC" "$HOME/.vscode/extensions/$VSCODE_EXT_DIRNAME"
            sync_vscode_registry "register" "$HOME/.vscode/extensions"
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
        ssh "${ssh_opts[@]}" "$SSH_TARGET" bash -s -- "$INSTALL_VIM" "$INSTALL_VSCODE" "$EXT_ID" "$EXT_VERSION" "$VSCODE_EXT_DIRNAME" <<'REMOTE_UNINSTALL'
set -euo pipefail
INSTALL_VIM="$1"
INSTALL_VSCODE="$2"
EXT_ID="$3"
EXT_VERSION="$4"
VSCODE_EXT_DIRNAME="$5"

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
            for d in "$base"/omniscript-* "$base"/"${EXT_ID}"-*; do
                if [[ -e "$d" || -L "$d" ]]; then
                    rm -rf "$d"
                    echo "  [remote removed] $d"
                fi
            done
            if command -v python3 >/dev/null 2>&1; then
                python3 - "$base" "$EXT_ID" <<'PYEOF'
import json, os, sys
base, ext_id = sys.argv[1:3]
ext_id_lower = ext_id.lower()
obs_p = os.path.join(base, ".obsolete")
reg_p = os.path.join(base, "extensions.json")
if os.path.isfile(obs_p):
    try:
        obs = json.load(open(obs_p, "r", encoding="utf-8"))
        if isinstance(obs, dict):
            c = {k: v for k, v in obs.items() if not (k.lower().startswith(ext_id_lower + "-") or k.lower().startswith("omniscript-"))}
            if c != obs:
                json.dump(c, open(obs_p + ".tmp", "w", encoding="utf-8"))
                os.replace(obs_p + ".tmp", obs_p)
    except Exception:
        pass
if os.path.isfile(reg_p):
    try:
        entries = json.load(open(reg_p, "r", encoding="utf-8"))
        if isinstance(entries, list):
            f = [e for e in entries if not (isinstance(e, dict) and str(e.get("identifier", {}).get("id", "")).lower() == ext_id_lower)]
            if f != entries:
                json.dump(f, open(reg_p + ".tmp", "w", encoding="utf-8"), separators=(",", ":"))
                os.replace(reg_p + ".tmp", reg_p)
    except Exception:
        pass
PYEOF
            fi
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
EXT_ID="$4"
EXT_VERSION="$5"

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

sync_remote_vscode_registry() {
    local base="$1"
    if command -v python3 >/dev/null 2>&1; then
        python3 - "$base" "$EXT_ID" "$EXT_VERSION" "$VSCODE_EXT_DIRNAME" <<'PYEOF'
import json, os, sys, time
base, ext_id, ext_ver, dirname = sys.argv[1:5]
ext_id_lower = ext_id.lower()
obs_p = os.path.join(base, ".obsolete")
reg_p = os.path.join(base, "extensions.json")
if os.path.isfile(obs_p):
    try:
        obs = json.load(open(obs_p, "r", encoding="utf-8"))
        if isinstance(obs, dict):
            c = {k: v for k, v in obs.items() if not (k.lower().startswith(ext_id_lower + "-") or k.lower().startswith("omniscript-"))}
            if c != obs:
                json.dump(c, open(obs_p + ".tmp", "w", encoding="utf-8"))
                os.replace(obs_p + ".tmp", obs_p)
    except Exception:
        pass
if os.path.isfile(reg_p):
    try:
        entries = json.load(open(reg_p, "r", encoding="utf-8"))
        if isinstance(entries, list):
            f = [e for e in entries if not (isinstance(e, dict) and str(e.get("identifier", {}).get("id", "")).lower() == ext_id_lower)]
            f.append({
                "identifier": {"id": ext_id},
                "version": ext_ver,
                "location": {"$mid": 1, "path": os.path.join(base, dirname), "scheme": "file"},
                "relativeLocation": dirname,
                "metadata": {"installedTimestamp": int(time.time() * 1000), "pinned": True, "source": "vsix"},
            })
            json.dump(f, open(reg_p + ".tmp", "w", encoding="utf-8"), separators=(",", ":"))
            os.replace(reg_p + ".tmp", reg_p)
            print(f"  [remote registry] registered {ext_id}@{ext_ver} in {reg_p}")
    except Exception:
        pass
PYEOF
    fi
}

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
            for old in "$base"/omniscript-* "$base"/"${EXT_ID}"-*; do
                [ -e "$old" ] || [ -L "$old" ] && rm -rf "$old"
            done
            cp -a "$TMP_DIR/vscode-omniscript" "$base/$VSCODE_EXT_DIRNAME"
            sync_remote_vscode_registry "$base"
            echo "  [remote installed] $base/$VSCODE_EXT_DIRNAME"
            installed_any=1
        fi
    done
    if [ "$installed_any" -eq 0 ]; then
        mkdir -p "$HOME/.vscode-server/extensions"
        cp -a "$TMP_DIR/vscode-omniscript" "$HOME/.vscode-server/extensions/$VSCODE_EXT_DIRNAME"
        sync_remote_vscode_registry "$HOME/.vscode-server/extensions"
        echo "  [remote installed] ~/.vscode-server/extensions/$VSCODE_EXT_DIRNAME"
    fi
fi
REMOTE_INSTALL
)"
    local remote_b64
    remote_b64="$(printf '%s' "$remote_script" | base64 | tr -d '\n')"

    tar -C "$SCRIPT_DIR/editors" -czf - "${tar_paths[@]}" | \
        ssh "${ssh_opts[@]}" "$SSH_TARGET" \
            "bash -c \"\$(printf '%s' '$remote_b64' | base64 -d)\" _ '$INSTALL_VIM' '$INSTALL_VSCODE' '$VSCODE_EXT_DIRNAME' '$EXT_ID' '$EXT_VERSION'"
    echo "Done. Reload VS Code / Remote-SSH window to activate."
}

if [[ -n "$SSH_TARGET" ]]; then
    run_ssh
else
    run_local
fi
