# Publishing and maintaining the MarretaLang extension

Maintainer notes for `docs/editors/vscode`. This file is not part of the public marketplace
listing (it is excluded from the packaged VSIX via `.vscodeignore`).

## Publishing

The extension is published through the manual `release-vscode.yml` GitHub workflow (Spec 070), not
by editing the marketplace pages directly. The workflow:

- is `workflow_dispatch` only, triggered on a `vscode-v*` tag,
- guards that the tag equals the `version` in `package.json`, so bump `version` before tagging,
- publishes to Open VSX and the VS Code Marketplace, each channel gated on its secret
  (`OVSX_TOKEN`, `VSCE_PAT`), with a self-verify step.

A merge to `main` does not change the live listing. The listing (README, metadata, screenshot) goes
live on the next publish, so a content change here is only visible to users after that publish runs.

### One-time prerequisites (Spec 070)

Before enabling a channel, create the namespace/publisher and set its secret:

- Open VSX: `npx ovsx create-namespace MarretaTeam`, then set `OVSX_TOKEN`.
- VS Code Marketplace: create the `MarretaTeam` publisher at
  <https://marketplace.visualstudio.com/manage>, then set `VSCE_PAT`. The publisher id must match
  `publisher` in `package.json`.

### Local packaging (to inspect the VSIX)

```bash
npx @vscode/vsce package --no-dependencies
```

`--no-dependencies` is used because the extension only uses the VS Code API and Node builtins (no
runtime dependencies). The local package is for inspection. The Markdown structure renders locally,
but image resolution does not (the marketplace rewrites relative image URLs and Open VSX diverges),
which is why the README references the screenshot by an absolute `raw.githubusercontent.com` URL,
verified by opening the URL directly.

## When the language surface changes

The grammar is scoped to the language as it exists. If the surface changes, update:

1. `syntaxes/marreta.tmLanguage.json`
2. `language-configuration.json`
3. `snippets/marreta.json`
4. `package.json` version

Keep the packaged artifact version aligned with the runtime release.

## Grammar and scope coverage (reference)

The grammar covers the current surface:

- `task`, `route`, `schema`, `auth`, `scenario`
- `if / else`, `while`, `match`, `transaction`, `raise / rescue`
- `db`, `doc`, `cache`, `queue`, `topic`, `time`, `http_client`
- `math`, `fs`, `json`, `base64`, `uuid`, `feature`, `log`
- temporal and contract types: `instant`, `date`, `time`, `duration`, `interval`, `decimal`
- queue consumers: `on queue`, `on topic`
- API scenario testing constructs: `given`, `when`, `then`, `returns`, `anything`
