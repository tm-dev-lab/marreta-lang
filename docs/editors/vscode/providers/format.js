const vscode = require("vscode");
const { toolingContext, runText, showToolingError } = require("../client/marretaCli");

function createFormattingProvider() {
  return {
    async provideDocumentFormattingEdits(document) {
      const context = toolingContext(document);

      try {
        const formatted = await runText(
          ["fmt", "--stdin", "--file", context.file],
          { cwd: context.root, stdin: document.getText() }
        );
        const fullRange = new vscode.Range(
          document.positionAt(0),
          document.positionAt(document.getText().length)
        );
        return [vscode.TextEdit.replace(fullRange, formatted)];
      } catch (error) {
        showToolingError(error);
        return [];
      }
    },
  };
}

module.exports = { createFormattingProvider };

