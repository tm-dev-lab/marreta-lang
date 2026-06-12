# MarretaLang for VS Code

Editor support for [Marreta](https://marreta.dev), a language for building REST APIs. You get
syntax highlighting, completions, inline diagnostics, formatting, and cross-file navigation for
`.marreta` files, all backed by the `marreta` CLI.

![Marreta completions with inline documentation in VS Code](https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/docs/editors/vscode/images/completion.png)

## Requirements

This extension is a thin client over the **`marreta` CLI**, which provides every bit of the
language intelligence. Install the CLI first, otherwise the extension has nothing to talk to and
the editor features stay inactive.

1. Install the `marreta` CLI. See [Install the editor extension](https://marreta.dev/docs/how-to/install-the-editor-extension)
   for the per-editor steps.
2. Install this extension.
3. Open a `.marreta` file.

If the CLI is not on your `PATH`, point the extension at it:

```json
{ "marreta.path": "/path/to/marreta" }
```

## Features

- **Syntax highlighting** for the full language surface, plus a mallet file icon (and an opt-in
  "Marreta File Icons" theme for the Explorer).
- **Completions, hover, and inline diagnostics** as you type, with format-on-save.
- **Symbols and navigation**: document and workspace symbols, and cross-file go-to-definition for
  tasks, schemas, and auth providers.
- **Code actions and commands**: a "Remove unused variable" quick-fix, CodeLens to run scenarios
  (and to serve the project from `app.marreta`), palette commands (Serve, Test, Lint, Format
  Document, Doctor, Init), and a status bar item showing the CLI version and tooling health.
- **Single files too**: a standalone `.marreta` file with no `app.marreta` still gets
  intelligence, and a missing CLI surfaces an actionable notification instead of failing silently.

## Settings

- `marreta.path`: path to the `marreta` CLI binary (default `marreta`).
- `marreta.diagnostics.onChange`: run diagnostics while editing (default true, they also run on
  save).
- `marreta.tooling.debounceMs`: debounce delay for CLI-backed tooling calls (default 150).

## How it works

Every bit of language understanding comes from the installed `marreta` CLI. The extension routes
requests to it and renders the results, with no editor-side parser, so the editor and the CLI never
disagree and the intelligence always matches the runtime you have. It is intentionally not a
language-server daemon, a refactoring engine, or a debugger.

## Links

- Documentation: <https://marreta.dev/docs>
- Source and issues: <https://github.com/tm-dev-lab/marreta-lang>
