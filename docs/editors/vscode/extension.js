const vscode = require("vscode");

const { createCompletionProvider } = require("./providers/completion");
const { createDefinitionProvider } = require("./providers/definition");
const { createDiagnostics } = require("./providers/diagnostics");
const { createFormattingProvider } = require("./providers/format");
const { createHoverProvider } = require("./providers/hover");
const {
  createDocumentSymbolProvider,
  createWorkspaceSymbolProvider,
} = require("./providers/symbols");
const { createCodeLensProvider } = require("./providers/codeLens");
const {
  createCodeActionProvider,
  CODE_ACTION_KINDS,
} = require("./providers/codeActions");
const { registerCommands } = require("./providers/commands");
const { createStatusBar } = require("./providers/statusBar");
const {
  createSemanticTokensProvider,
  semanticTokensLegend,
} = require("./providers/semanticTokens");

function activate(context) {
  const selector = { language: "marreta", scheme: "file" };

  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      selector,
      createCompletionProvider(),
      "."
    ),
    vscode.languages.registerDefinitionProvider(selector, createDefinitionProvider()),
    vscode.languages.registerHoverProvider(selector, createHoverProvider()),
    vscode.languages.registerDocumentFormattingEditProvider(
      selector,
      createFormattingProvider()
    ),
    vscode.languages.registerDocumentSymbolProvider(
      selector,
      createDocumentSymbolProvider()
    ),
    vscode.languages.registerWorkspaceSymbolProvider(
      createWorkspaceSymbolProvider()
    ),
    vscode.languages.registerCodeLensProvider(selector, createCodeLensProvider()),
    vscode.languages.registerCodeActionsProvider(selector, createCodeActionProvider(), {
      providedCodeActionKinds: CODE_ACTION_KINDS,
    }),
    vscode.languages.registerDocumentSemanticTokensProvider(
      selector,
      createSemanticTokensProvider(),
      semanticTokensLegend
    )
  );

  registerCommands(context);
  createStatusBar(context);
  createDiagnostics(context);
}

function deactivate() {}

module.exports = { activate, deactivate };
