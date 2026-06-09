const path = require("path");
const vscode = require("vscode");
const { toolingContext, runJson, showToolingError } = require("../client/marretaCli");

// Cross-file go-to-definition. Thin client of `marreta tooling definition`: the CLI
// resolves the reference (task/schema/auth) at the cursor and returns its
// declaration location; this provider only maps that to a VS Code Location.
function createDefinitionProvider() {
  return {
    async provideDefinition(document, position) {
      const context = toolingContext(document);

      try {
        const target = await runJson(
          [
            "tooling",
            "definition",
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
        if (!target || !target.file) {
          return undefined;
        }
        const uri = vscode.Uri.file(
          path.isAbsolute(target.file)
            ? target.file
            : path.join(context.root, target.file)
        );
        const pos = new vscode.Position(
          Math.max(0, (target.line || 1) - 1),
          Math.max(0, (target.column || 1) - 1)
        );
        return new vscode.Location(uri, pos);
      } catch (error) {
        showToolingError(error);
        return undefined;
      }
    },
  };
}

module.exports = { createDefinitionProvider };
