# MarretaLang VS Code Support

This folder contains the lightweight VS Code bundle for MarretaLang.

Current scope:

- `.marreta` file association, with a purple mallet icon on editor tabs (and an
  opt-in "Marreta File Icons" theme for the Explorer)
- basic indentation rules for block-oriented syntax
- TextMate syntax highlighting for the current language surface
- snippets for common declarations (route, task, schema, reply, fail, match,
  if/else, auth, take, pipeline, http_client, db, scenario, on queue/topic)
- CLI-backed completions, hover, diagnostics (with full-token spans), formatting,
  document and workspace symbols, and cross-file go-to-definition for tasks,
  schemas, and auth providers
- a "Remove unused variable" quick-fix, CodeLens to run scenarios (and a single
  Serve lens on the project bootstrap `app.marreta`), palette commands (Serve, Test,
  Lint, Format Document, Doctor, Init), and a status bar item showing the CLI version
  and tooling health. "Marreta: Format Document" formats from the palette (handy when
  the Shift+Alt+F shortcut conflicts with a keyboard-layout mnemonic)
- standalone `.marreta` files (no `app.marreta`) still get intelligence, and a
  missing CLI surfaces an actionable notification instead of failing silently

All language understanding comes from the `marreta` CLI: the extension is a thin
client that routes requests and renders results, with no editor-side parser.

The grammar is intentionally scoped to the language as it exists today, including:

- `task`, `route`, `schema`, `auth`, `scenario`
- `if / else`, `while`, `match`, `transaction`, `raise / rescue`
- `db`, `doc`, `cache`, `queue`, `topic`, `time`, `http_client`
- `math`, `fs`, `json`, `base64`, `uuid`, `feature`, `log`
- temporal and contract types: `instant`, `date`, `time`, `duration`, `interval`, `decimal`
- queue consumers: `on queue`, `on topic`
- API scenario testing constructs: `given`, `when`, `then`, `returns`, `anything`

Editor intelligence is provided by the installed `marreta` CLI. Configure the
binary path if it is not available on `PATH`:

```json
{
  "marreta.path": "/path/to/marreta"
}
```

What this bundle does not try to be yet:

- a long-running language server daemon
- a refactoring engine
- a debugger integration
- an editor-side parser implementation

If the language surface changes, update:

1. `syntaxes/marreta.tmLanguage.json`
2. `language-configuration.json`
3. `snippets/marreta.json`
4. `package.json` version

If you publish a new VSIX, keep the generated artifact versions aligned with the package version.

## Publishing to the Marketplace (maintainers)

This bundle is self-contained: run everything below from this directory
(`docs/editors/vscode`). Required manifest fields, the icon, the README, and the
`LICENSE` are already in place.

### One-time setup

1. Create/use a Microsoft account and an Azure DevOps organization
   (`dev.azure.com`).
2. Generate a Personal Access Token: Azure DevOps → User Settings → Personal Access
   Tokens → New Token → Organization **All accessible**, Scope **Marketplace →
   Manage**. Save the token.
3. Create the publisher at <https://marketplace.visualstudio.com/manage>. Its ID
   must match `publisher` in `package.json` (currently `MarretaTeam`).

### Publish

```bash
npm i -g @vscode/vsce          # or use: npx @vscode/vsce <cmd>
vsce login MarretaTeam          # paste the PAT once
vsce publish --no-dependencies  # package and publish the current version
```

- Bump and publish in one step: `vsce publish patch` (or `minor` / `major`).
- Package a `.vsix` without publishing (to upload manually on the site):
  `vsce package --no-dependencies`.
- `--no-dependencies` is used because the extension only uses the VS Code API and
  Node builtins (no runtime dependencies).

### Notes

- Each publish needs a new `version`; keep it aligned with the runtime release.
- The extension requires the `marreta` CLI on `PATH` (or `marreta.path` set). Make
  sure that is clear to users — it is stated above.
- Also publishing to [Open VSX](https://open-vsx.org) (for VSCodium, Gitpod, Cursor,
  …) is optional: `npx ovsx publish --no-dependencies -p <OPENVSX_TOKEN>`.
