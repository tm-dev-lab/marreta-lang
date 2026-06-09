const vscode = require("vscode");
const { toolingContext, runJson, showToolingError } = require("../client/marretaCli");

const kindMap = {
  keyword: vscode.CompletionItemKind.Keyword,
  namespace: vscode.CompletionItemKind.Module,
  function: vscode.CompletionItemKind.Function,
  method: vscode.CompletionItemKind.Method,
  task: vscode.CompletionItemKind.Function,
  schema: vscode.CompletionItemKind.Class,
  route: vscode.CompletionItemKind.Interface,
};

function createCompletionProvider() {
  return {
    async provideCompletionItems(document, position) {
      const context = toolingContext(document);

      try {
        const items = await runJson(
          [
            "tooling",
            "completions",
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
        return (items || []).map(toCompletionItem);
      } catch (error) {
        showToolingError(error);
        return [];
      }
    },
  };
}

function toCompletionItem(item) {
  const completion = new vscode.CompletionItem(
    item.label,
    kindMap[item.kind] || vscode.CompletionItemKind.Text
  );
  completion.detail = item.detail;
  completion.documentation = new vscode.MarkdownString(item.documentation || "");
  completion.insertText = new vscode.SnippetString(item.insert_text || item.label);
  if (item.sort_text) {
    completion.sortText = item.sort_text;
  }
  return completion;
}

module.exports = { createCompletionProvider };

