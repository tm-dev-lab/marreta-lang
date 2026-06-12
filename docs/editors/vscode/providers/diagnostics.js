const vscode = require("vscode");
const path = require("path");
const {
  debounce,
  debounceMs,
  diagnosticsOnChange,
  toolingContext,
  runJson,
  showToolingError,
} = require("../client/marretaCli");

function createDiagnostics(context) {
  const collection = vscode.languages.createDiagnosticCollection("marreta");
  context.subscriptions.push(collection);

  const update = async (document) => {
    if (document.languageId !== "marreta") {
      return;
    }
    const project = toolingContext(document);

    try {
      const diagnostics = await runJson(
        ["lint", "--stdin", "--file", project.file, "--format", "json"],
        { cwd: project.root, stdin: document.getText(), allowNonZero: true }
      );
      collection.set(
        document.uri,
        (diagnostics || [])
          .filter((item) => diagnosticBelongsToDocument(item, project, document))
          .map(toDiagnostic)
      );
    } catch (error) {
      showToolingError(error);
    }
  };

  const debouncedUpdate = debounce(update, debounceMs());
  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument(update),
    vscode.workspace.onDidCloseTextDocument((document) => collection.delete(document.uri))
  );
  if (diagnosticsOnChange()) {
    context.subscriptions.push(
      vscode.workspace.onDidChangeTextDocument((event) => debouncedUpdate(event.document))
    );
  }

  for (const document of vscode.workspace.textDocuments) {
    update(document);
  }
}

function diagnosticBelongsToDocument(item, project, document) {
  if (!item.file) {
    return true;
  }

  const normalizedItem = normalizePath(item.file);
  const normalizedProjectFile = normalizePath(project.file);
  if (normalizedItem === normalizedProjectFile) {
    return true;
  }

  const absoluteItem = path.isAbsolute(item.file)
    ? normalizePath(item.file)
    : normalizePath(path.join(project.root, item.file));
  return absoluteItem === normalizePath(document.uri.fsPath);
}

function normalizePath(value) {
  return path.normalize(value).replace(/\\/g, "/");
}

function toDiagnostic(item) {
  const line = Math.max(0, (item.line || 1) - 1);
  const column = Math.max(0, (item.column || 1) - 1);
  const endLine = Math.max(0, (item.end_line || item.line || 1) - 1);
  let endColumn = Math.max(0, (item.end_column || item.column || 1) - 1);
  // Fall back to a single-character squiggle when the CLI reports no real span.
  if (endLine === line && endColumn <= column) {
    endColumn = column + 1;
  }
  const range = new vscode.Range(line, column, endLine, endColumn);
  const diagnostic = new vscode.Diagnostic(
    range,
    item.help ? `${item.message}\n${item.help}` : item.message,
    toSeverity(item.severity)
  );
  // The code carries a link to its docs anchor, so the editor shows "what is this and how do I fix
  // it" one hover away (Spec 071). The anchor matches the `### <code>` heading on the lint page.
  diagnostic.code = item.code
    ? {
        value: item.code,
        target: vscode.Uri.parse(`https://marreta.dev/docs/reference/lint#${item.code}`),
      }
    : undefined;
  diagnostic.source = "marreta";
  return diagnostic;
}

function toSeverity(severity) {
  switch (severity) {
    case "error":
      return vscode.DiagnosticSeverity.Error;
    case "warning":
      return vscode.DiagnosticSeverity.Warning;
    case "info":
      return vscode.DiagnosticSeverity.Information;
    default:
      return vscode.DiagnosticSeverity.Hint;
  }
}

module.exports = { createDiagnostics };
