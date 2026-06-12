# Listing images

Assets referenced by the marketplace listing (`../README.md`). Excluded from the packaged VSIX
(see `../.vscodeignore`), because the README references them by absolute
`raw.githubusercontent.com` URL so they resolve identically on the VS Code Marketplace and Open VSX
(Spec 074).

## Expected asset

- `completion.png` — a static screenshot of the extension in action: a `.marreta` file open in VS
  Code showing a completion popup or an inline diagnostic. This is the listing's visual floor (Spec
  074, AC4). Capture it, commit it here as `completion.png`, and push to `main` before the publish
  so the raw URL resolves on the live listing.

Animated GIFs (format-on-save, live completions) are a named follow-up, not required for the first
listing.
