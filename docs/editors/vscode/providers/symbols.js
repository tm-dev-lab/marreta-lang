const path = require("path");
const vscode = require("vscode");
const { toolingContext, runJson, showToolingError } = require("../client/marretaCli");

const kindMap = {
  route: vscode.SymbolKind.Interface,
  task: vscode.SymbolKind.Function,
  schema: vscode.SymbolKind.Class,
  auth: vscode.SymbolKind.Constant,
  consumer: vscode.SymbolKind.Event,
  scenario: vscode.SymbolKind.String,
};

function createDocumentSymbolProvider() {
  return {
    async provideDocumentSymbols(document) {
      const context = toolingContext(document);

      try {
        // `--stdin` makes a project-less file fall back to single-buffer symbols;
        // with a project the CLI returns project-wide symbols, filtered to this file.
        const symbols = await runJson(
          ["tooling", "symbols", "--stdin", "--file", context.file, "--format", "json"],
          { cwd: context.root, stdin: document.getText() }
        );
        return (symbols || [])
          .filter((symbol) => symbol.file === context.file)
          .map(toDocumentSymbol);
      } catch (error) {
        showToolingError(error);
        return [];
      }
    },
  };
}

function createWorkspaceSymbolProvider() {
  return {
    async provideWorkspaceSymbols(query) {
      const folders = vscode.workspace.workspaceFolders || [];
      const results = [];

      for (const folder of folders) {
        try {
          const symbols = await runJson(
            ["tooling", "symbols", "--format", "json"],
            { cwd: folder.uri.fsPath }
          );
          for (const symbol of symbols || []) {
            if (matchesQuery(symbol, query)) {
              results.push(toSymbolInformation(folder.uri.fsPath, symbol));
            }
          }
        } catch (error) {
          showToolingError(error);
        }
      }

      return results;
    },
  };
}

function toDocumentSymbol(symbol) {
  const line = Math.max(0, symbol.line - 1);
  const column = Math.max(0, symbol.column - 1);
  const range = new vscode.Range(line, column, line, column + symbol.name.length);
  return new vscode.DocumentSymbol(
    symbol.name,
    symbol.detail || "",
    kindMap[symbol.kind] || vscode.SymbolKind.Variable,
    range,
    range
  );
}

function toSymbolInformation(root, symbol) {
  const line = Math.max(0, symbol.line - 1);
  const column = Math.max(0, symbol.column - 1);
  const location = new vscode.Location(
    vscode.Uri.file(path.join(root, symbol.file)),
    new vscode.Position(line, column)
  );
  return new vscode.SymbolInformation(
    symbol.name,
    kindMap[symbol.kind] || vscode.SymbolKind.Variable,
    symbol.detail || "",
    location
  );
}

function matchesQuery(symbol, query) {
  const normalized = (query || "").toLowerCase();
  if (!normalized) {
    return true;
  }
  return (
    symbol.name.toLowerCase().includes(normalized) ||
    (symbol.detail || "").toLowerCase().includes(normalized)
  );
}

module.exports = { createDocumentSymbolProvider, createWorkspaceSymbolProvider };
