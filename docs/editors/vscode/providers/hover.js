const vscode = require("vscode");
const { toolingContext, runJson, showToolingError } = require("../client/marretaCli");

function createHoverProvider() {
  return {
    async provideHover(document, position) {
      const context = toolingContext(document);

      try {
        const hover = await runJson(
          [
            "tooling",
            "hover",
            "--stdin",
            "--file",
            context.file,
            "--line",
            String(position.line + 1),
            "--column",
            String(position.character + 1),
            "--format",
            "json",
          ],
          { cwd: context.root, stdin: document.getText() }
        );
        if (!hover) {
          return undefined;
        }
        const contents = (hover.contents || []).map((item) => {
          if (item.kind === "markdown") {
            return new vscode.MarkdownString(item.value || "");
          }
          return item.value || "";
        });
        return new vscode.Hover(contents, toRange(hover.range));
      } catch (error) {
        showToolingError(error);
        return undefined;
      }
    },
  };
}

function toRange(range) {
  if (!range) {
    return undefined;
  }
  return new vscode.Range(
    Math.max(0, range.start.line - 1),
    Math.max(0, range.start.column - 1),
    Math.max(0, range.end.line - 1),
    Math.max(0, range.end.column - 1)
  );
}

module.exports = { createHoverProvider };

