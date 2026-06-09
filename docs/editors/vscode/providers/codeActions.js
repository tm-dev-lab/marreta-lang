const vscode = require("vscode");

// Quick-fixes driven strictly by CLI diagnostics — no editor-side analysis. For an
// `unused_variable` diagnostic, offer to delete the flagged assignment line; the
// range comes from the CLI (code + span), the edit is purely mechanical.
function createCodeActionProvider() {
  return {
    provideCodeActions(document, _range, context) {
      const actions = [];
      for (const diagnostic of context.diagnostics) {
        if (diagnostic.source === "marreta" && diagnostic.code === "unused_variable") {
          const action = new vscode.CodeAction(
            "Remove unused variable",
            vscode.CodeActionKind.QuickFix
          );
          action.diagnostics = [diagnostic];
          const edit = new vscode.WorkspaceEdit();
          const line = document.lineAt(diagnostic.range.start.line);
          edit.delete(document.uri, line.rangeIncludingLineBreak);
          action.edit = edit;
          actions.push(action);
        }
      }
      return actions;
    },
  };
}

module.exports = {
  createCodeActionProvider,
  CODE_ACTION_KINDS: [vscode.CodeActionKind.QuickFix],
};
