const path = require("path");
const vscode = require("vscode");
const { binaryPath, findProjectRoot } = require("../client/marretaCli");

// Palette commands. Thin wrappers that run the CLI in an integrated terminal in the
// right working directory; no logic beyond launching `marreta`.

function workingDir() {
  const editor = vscode.window.activeTextEditor;
  if (editor && editor.document.uri.scheme === "file") {
    const root = findProjectRoot(editor.document.uri);
    if (root) {
      return root;
    }
    return path.dirname(editor.document.uri.fsPath);
  }
  const folders = vscode.workspace.workspaceFolders || [];
  return folders.length ? folders[0].uri.fsPath : undefined;
}

function runInTerminal(name, args, cwd) {
  const terminal = vscode.window.createTerminal({ name, cwd });
  terminal.show();
  terminal.sendText(`${binaryPath()} ${args.join(" ")}`);
}

function registerCommands(context) {
  const register = (id, handler) =>
    context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  register("marreta.serve", () => runInTerminal("Marreta Serve", ["serve"], workingDir()));
  register("marreta.test", () => runInTerminal("Marreta Test", ["test"], workingDir()));
  register("marreta.lint", () => runInTerminal("Marreta Lint", ["lint"], workingDir()));
  // Formats the active editor through the registered DocumentFormattingEditProvider,
  // so it works from the palette without relying on the Shift+Alt+F shortcut.
  register("marreta.format", () =>
    vscode.commands.executeCommand("editor.action.formatDocument")
  );
  register("marreta.doctor", () => runInTerminal("Marreta Doctor", ["doctor"], workingDir()));
  register("marreta.init", async () => {
    const target = await vscode.window.showInputBox({
      prompt: "Project path for marreta init",
      placeHolder: "my-api",
    });
    if (target) {
      runInTerminal("Marreta Init", ["init", target], workingDir());
    }
  });
  // Used by the CodeLens "Run scenario" action.
  register("marreta.runScenario", (name) =>
    runInTerminal("Marreta Test", ["test", "--filter", JSON.stringify(name)], workingDir())
  );
}

module.exports = { registerCommands };
