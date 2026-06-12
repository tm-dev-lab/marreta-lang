const vscode = require("vscode");

// Quick-fixes driven strictly by CLI diagnostics — no editor-side analysis. The edits are purely
// mechanical: a real fix from the diagnostic span (delete the unused assignment), and a suppression
// fix available for every marreta diagnostic. The suppress action is listed after any real fix and
// is never preferred, so suppressing is never the default action (Spec 071).
function createCodeActionProvider() {
  return {
    provideCodeActions(document, _range, context) {
      const actions = [];
      for (const diagnostic of context.diagnostics) {
        if (diagnostic.source !== "marreta") {
          continue;
        }
        const code = diagnosticCode(diagnostic);

        // Real fix first.
        if (code === "unused_variable") {
          const action = new vscode.CodeAction(
            "Remove unused variable",
            vscode.CodeActionKind.QuickFix
          );
          action.diagnostics = [diagnostic];
          action.isPreferred = true;
          const edit = new vscode.WorkspaceEdit();
          const line = document.lineAt(diagnostic.range.start.line);
          edit.delete(document.uri, line.rangeIncludingLineBreak);
          action.edit = edit;
          actions.push(action);
        }

        // Suppression fix, after any real fix, never preferred.
        if (code) {
          const suppress = new vscode.CodeAction(
            `Suppress ${code} on this line`,
            vscode.CodeActionKind.QuickFix
          );
          suppress.diagnostics = [diagnostic];
          const targetLine = document.lineAt(diagnostic.range.start.line);
          const indent = targetLine.text.match(/^\s*/)[0];
          const edit = new vscode.WorkspaceEdit();
          edit.insert(
            document.uri,
            new vscode.Position(diagnostic.range.start.line, 0),
            `${indent}# marreta: allow ${code}\n`
          );
          suppress.edit = edit;
          actions.push(suppress);
        }
      }
      return actions;
    },
  };
}

// The diagnostic code is an object ({ value, target }) so the editor can link it to its docs anchor.
function diagnosticCode(diagnostic) {
  return diagnostic.code && typeof diagnostic.code === "object"
    ? diagnostic.code.value
    : diagnostic.code;
}

module.exports = {
  createCodeActionProvider,
  CODE_ACTION_KINDS: [vscode.CodeActionKind.QuickFix],
};
