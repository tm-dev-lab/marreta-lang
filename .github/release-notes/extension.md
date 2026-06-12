## VS Code Extension __TAG__

Editor support for `.marreta` files: syntax highlighting, completion, hover,
go-to-definition, diagnostics, and formatting.

The extension is a thin client over the Marreta CLI, so install the binary first:

```bash
curl -fsSL https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/install.sh | sh
```

### Install this build

Download `marreta-extension.vsix` below, then open the command palette (`Ctrl+Shift+P`, macOS
`Cmd+Shift+P`), run **Extensions: Install from VSIX**, and select the file. This works the same in
VS Code, Cursor, VSCodium, and Windsurf, with no CLI on your `PATH`.

If you prefer the command line: `code --install-extension marreta-extension.vsix` (Cursor:
`cursor --install-extension marreta-extension.vsix`).

Once the extension is published to the registries, you will also find it by searching for
**MarretaLang** in the VS Code Marketplace (VS Code) or Open VSX (Cursor and forks).

Full guide: https://marreta.dev/docs/how-to/install-the-editor-extension
