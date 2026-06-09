const path = require("path");
const vscode = require("vscode");
const { toolingContext, runJson, showToolingError } = require("../client/marretaCli");

// CodeLens anchored on CLI-provided symbols (no editor-side parsing): "Run scenario"
// over each scenario, and a single "Serve" lens at the top of the project bootstrap
// (app.marreta). Serve is a project-level action, so it is intentionally not shown
// per route (N routes do not mean N servers).
function createCodeLensProvider() {
  return {
    async provideCodeLenses(document) {
      const context = toolingContext(document);
      const lenses = [];

      if (path.basename(document.uri.fsPath) === "app.marreta") {
        lenses.push(
          new vscode.CodeLens(new vscode.Range(0, 0, 0, 0), {
            title: "$(server) Serve",
            command: "marreta.serve",
          })
        );
      }

      try {
        const symbols = await runJson(
          ["tooling", "symbols", "--stdin", "--file", context.file, "--format", "json"],
          { cwd: context.root, stdin: document.getText() }
        );
        for (const symbol of symbols || []) {
          if (symbol.file !== context.file || symbol.kind !== "scenario") {
            continue;
          }
          const line = Math.max(0, (symbol.line || 1) - 1);
          lenses.push(
            new vscode.CodeLens(new vscode.Range(line, 0, line, 0), {
              title: "$(play) Run scenario",
              command: "marreta.runScenario",
              arguments: [symbol.name],
            })
          );
        }
        return lenses;
      } catch (error) {
        showToolingError(error);
        return lenses;
      }
    },
  };
}

module.exports = { createCodeLensProvider };
