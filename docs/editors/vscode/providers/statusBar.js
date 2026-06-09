const vscode = require("vscode");
const { runText } = require("../client/marretaCli");

// Status bar item showing the resolved `marreta` version and tooling health. Pure
// CLI invocation (`marreta --version`); no language analysis.
function createStatusBar(context) {
  const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 0);
  item.command = "marreta.doctor";
  context.subscriptions.push(item);

  const refresh = async () => {
    try {
      const out = await runText(["--version"], {});
      const version = (out || "").trim().split(/\s+/).pop() || "";
      item.text = `$(tools) Marreta ${version}`.trim();
      item.tooltip = "Marreta CLI is available. Click to run doctor.";
      item.backgroundColor = undefined;
    } catch (error) {
      item.text = "$(warning) Marreta";
      item.tooltip = `Marreta CLI not found: ${error.message}. Set 'marreta.path'.`;
      item.backgroundColor = new vscode.ThemeColor("statusBarItem.warningBackground");
      item.command = "workbench.action.openSettings";
    }
    item.show();
  };

  refresh();
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("marreta.path")) {
        refresh();
      }
    })
  );
}

module.exports = { createStatusBar };
