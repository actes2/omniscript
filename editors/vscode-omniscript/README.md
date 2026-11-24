# OmniScript VS Code Extension (`vscode-omniscript`)

Syntax highlighting, auto-closing/indentation rules, and snippets for `.omni` polyglot source files (Python, C, C#, Java, Rust, Go, JavaScript, and bare-metal Raspberry Pi Pico HAL).

## Quick Local Install (Symlink — Zero Packaging Needed)
```bash
# Standard VS Code
mkdir -p ~/.vscode/extensions
ln -sfn "$(pwd)/editors/vscode-omniscript" ~/.vscode/extensions/omniscript-0.2.2

# Arch Linux / Code-OSS
mkdir -p ~/.vscode-oss/extensions
ln -sfn "$(pwd)/editors/vscode-omniscript" ~/.vscode-oss/extensions/omniscript-0.2.2
```
Reload VS Code (`Ctrl+Shift+P` -> `Developer: Reload Window`).

## Optional `.vsix` Packaging
```bash
cd editors/vscode-omniscript
npx @vscode/vsce package
code --install-extension omniscript-0.2.2.vsix
```
