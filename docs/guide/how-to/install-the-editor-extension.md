---
title: "Install the editor extension"
category: how-to
slug: "how-to/install-the-editor-extension"
summary: "Install the Marreta CLI and the MarretaLang editor extension in VS Code or Cursor, so .marreta files get syntax, completion, diagnostics, and formatting."
---

# Install the editor extension

The MarretaLang extension brings syntax highlighting, completion, hover, go-to-definition,
diagnostics, and formatting to `.marreta` files. It is a thin client over the `marreta` CLI: the
editor features call the binary under the hood, so you install the binary first and the extension
second.

## 1. Install the Marreta CLI

```bash
curl -fsSL https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/install.sh | sh
```

This installs the `marreta` binary. Confirm it is on your `PATH`:

```bash
marreta --version
```

If the command is not found, add the install location to your `PATH`, or note the full path for
step 3. Windows is supported through WSL: run the same command inside your WSL distribution.

## 2. Install the extension

### From a release VSIX (works today, every editor)

Every extension release attaches a `.vsix` file. Download the latest one from the
[GitHub releases](https://github.com/tm-dev-lab/marreta-lang/releases) (the
`VS Code Extension vscode-vX.Y.Z` entries), then install it:

- **VS Code**: run `code --install-extension marreta-extension.vsix`, or open the Extensions view,
  use the `...` menu, and choose **Install from VSIX**.
- **Cursor**: open the Extensions view, use the `...` menu, and choose **Install from VSIX**.

The same VSIX works in any VS Code-compatible editor (VSCodium, Windsurf, and others).

### From the marketplace

Once MarretaLang is published to the registries, you will be able to install it by name from the
Extensions view, without downloading a file:

- **VS Code** pulls from the Visual Studio Marketplace. Search for **MarretaLang** and install.
- **Cursor**, **VSCodium**, and **Windsurf** pull from Open VSX. Search for **MarretaLang** and
  install.

## 3. Point the extension at the binary (only if needed)

The extension calls `marreta` on your `PATH` by default. If the binary lives somewhere else, or the
extension reports that the CLI was not found, set its path in your settings:

```json
{
  "marreta.path": "/absolute/path/to/marreta"
}
```

Open a `.marreta` file and the editor features activate. If anything looks off, run
[`marreta doctor`](../reference/cli.md) to check your setup.
