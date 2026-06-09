const cp = require("child_process");
const fs = require("fs");
const path = require("path");
const vscode = require("vscode");

function binaryPath() {
  return vscode.workspace
    .getConfiguration("marreta")
    .get("path", "marreta");
}

function debounceMs() {
  return vscode.workspace
    .getConfiguration("marreta")
    .get("tooling.debounceMs", 150);
}

function diagnosticsOnChange() {
  return vscode.workspace
    .getConfiguration("marreta")
    .get("diagnostics.onChange", true);
}

function findProjectRoot(documentUri) {
  let dir = path.dirname(documentUri.fsPath);
  while (true) {
    if (fs.existsSync(path.join(dir, "app.marreta"))) {
      return dir;
    }
    const parent = path.dirname(dir);
    if (parent === dir) {
      return undefined;
    }
    dir = parent;
  }
}

function relativeFile(root, documentUri) {
  return path.relative(root, documentUri.fsPath).replace(/\\/g, "/");
}

function runJson(args, options) {
  return new Promise((resolve, reject) => {
    const child = cp.spawn(binaryPath(), args, {
      cwd: options.cwd,
      stdio: ["pipe", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code !== 0 && !options.allowNonZero) {
        reject(new Error(stderr || `marreta exited with code ${code}`));
        return;
      }
      try {
        resolve(stdout.trim() ? JSON.parse(stdout) : null);
      } catch (error) {
        reject(new Error(`invalid Marreta JSON response: ${error.message}`));
      }
    });
    if (options.stdin !== undefined) {
      child.stdin.write(options.stdin);
    }
    child.stdin.end();
  });
}

function runText(args, options) {
  return new Promise((resolve, reject) => {
    const child = cp.spawn(binaryPath(), args, {
      cwd: options.cwd,
      stdio: ["pipe", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code !== 0 && !options.allowNonZero) {
        reject(new Error(stderr || `marreta exited with code ${code}`));
        return;
      }
      resolve(stdout);
    });
    if (options.stdin !== undefined) {
      child.stdin.write(options.stdin);
    }
    child.stdin.end();
  });
}

function projectContext(document) {
  const root = findProjectRoot(document.uri);
  if (!root) {
    return undefined;
  }
  return {
    root,
    file: relativeFile(root, document.uri),
  };
}

// Tooling context for a single document. Uses the project root + relative file when
// the document belongs to a Marreta project, and otherwise falls back to the
// document's own directory and basename so standalone .marreta files still get
// CLI-backed intelligence (the CLI supports project-less --stdin).
function toolingContext(document) {
  const root = findProjectRoot(document.uri);
  if (root) {
    return { root, file: relativeFile(root, document.uri), hasProject: true };
  }
  return {
    root: path.dirname(document.uri.fsPath),
    file: path.basename(document.uri.fsPath),
    hasProject: false,
  };
}

let notifiedMissingBinary = false;

function showToolingError(error) {
  const missing =
    error && (error.code === "ENOENT" || /ENOENT|not found/i.test(error.message || ""));
  if (missing && !notifiedMissingBinary) {
    notifiedMissingBinary = true;
    const configured = binaryPath();
    vscode.window
      .showErrorMessage(
        `Marreta CLI not found (tried '${configured}'). Set 'marreta.path' to the binary.`,
        "Open Settings"
      )
      .then((choice) => {
        if (choice === "Open Settings") {
          vscode.commands.executeCommand("workbench.action.openSettings", "marreta.path");
        }
      });
  }
  console.warn(`[marreta] tooling error: ${error.message}`);
}

function debounce(fn, delay) {
  let timer;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), delay);
  };
}

module.exports = {
  binaryPath,
  debounce,
  debounceMs,
  diagnosticsOnChange,
  findProjectRoot,
  projectContext,
  toolingContext,
  runJson,
  runText,
  showToolingError,
};
