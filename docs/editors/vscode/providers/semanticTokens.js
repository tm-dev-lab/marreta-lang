const vscode = require("vscode");
const { toolingContext, runJson } = require("../client/marretaCli");

// Spec 061 file-namespaces: `file.task` is colored like the built-in namespaces
// (db, cache, topic, ...). Those are colored by the static TextMate grammar from a
// fixed list; file-namespaces are project-defined, so they need semantic tokens.
//
// The two standard token types below are mapped to the same TextMate scopes the
// grammar uses for built-in namespaces (see package.json `semanticTokenScopes`), so
// the colors match the native ones in any theme.
const TOKEN_NAMESPACE = 0;
const TOKEN_FUNCTION = 1;
const semanticTokensLegend = new vscode.SemanticTokensLegend(
  ["namespace", "function"],
  []
);

// `ns.method` (with optional surrounding whitespace), anywhere on a code line.
const NS_CALL = /([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)/g;

function createSemanticTokensProvider() {
  return {
    async provideDocumentSemanticTokens(document) {
      const context = toolingContext(document);

      let symbols;
      try {
        symbols = await runJson(
          ["tooling", "symbols", "--stdin", "--file", context.file, "--format", "json"],
          { cwd: context.root, stdin: document.getText() }
        );
      } catch (error) {
        // Coloring is best-effort; never surface tooling errors here.
        return null;
      }

      // namespace -> Set(exported task names). Only exported tasks are reachable
      // cross-file, so only they (and their owning namespace) get colored.
      const namespaces = new Map();
      for (const symbol of symbols || []) {
        if (symbol.kind === "task" && symbol.exported && symbol.namespace) {
          if (!namespaces.has(symbol.namespace)) {
            namespaces.set(symbol.namespace, new Set());
          }
          namespaces.get(symbol.namespace).add(symbol.name);
        }
      }

      const builder = new vscode.SemanticTokensBuilder(semanticTokensLegend);
      if (namespaces.size === 0) {
        return builder.build();
      }

      const lines = document.getText().split(/\r?\n/);
      for (let lineNo = 0; lineNo < lines.length; lineNo++) {
        const code = blankStringsAndComments(lines[lineNo]);
        NS_CALL.lastIndex = 0;
        let match;
        while ((match = NS_CALL.exec(code)) !== null) {
          const ns = match[1];
          const method = match[2];
          const exported = namespaces.get(ns);
          if (!exported) {
            continue;
          }
          builder.push(lineNo, match.index, ns.length, TOKEN_NAMESPACE, 0);
          if (exported.has(method)) {
            const methodStart = match.index + match[0].length - method.length;
            builder.push(lineNo, methodStart, method.length, TOKEN_FUNCTION, 0);
          }
        }
      }
      return builder.build();
    },
  };
}

// Replaces string contents and trailing comments with spaces, preserving column
// positions so token offsets stay accurate. This keeps `ns.task` references inside
// strings (`"core.x"`) or comments (`# core.x`) from being colored.
function blankStringsAndComments(line) {
  let out = "";
  let inString = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (inString) {
      out += " ";
      if (ch === '"' && line[i - 1] !== "\\") {
        inString = false;
      }
      continue;
    }
    if (ch === '"') {
      inString = true;
      out += " ";
      continue;
    }
    if (ch === "#") {
      out += " ".repeat(line.length - i);
      break;
    }
    out += ch;
  }
  return out;
}

module.exports = { createSemanticTokensProvider, semanticTokensLegend };
