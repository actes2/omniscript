// Zero-dependency VS Code / Code-OSS extension for OmniScript (.omni)
// Features:
//   1. Fast (~10ms) zero-LLM diagnostics (`omnic check --json`): red errors + blue LLM-inferred hints
//   2. LLVM IR Hover Provider: hover over user functions, extern/HAL calls, builtins, or LLM-inferred lines
//   3. Polyglot IntelliSense Completion Provider (Python, Rust, C#, Java, JS, Go, C/C++, Pico HAL) w/ auto-imports
//   4. Import CodeLens: 1-click live package lookup (PyPI, npm, crates.io, Maven, NuGet, Go) -> omni-requirements.txt

const vscode = require("vscode");
const cp = require("child_process");
const fs = require("fs");
const https = require("https");
const path = require("path");

// Builtin OmniScript universal runtime symbol -> LLVM IR hover catalog
const BUILTIN_LLVM_HOVERS = {
  print: {
    sig: "print(value: str | int | float)",
    ir: "call void @__omni_print_str(i8* %s)\ncall void @__omni_print_int(i64 %n)\ncall void @__omni_print_float(double %f)",
    note: "Target-universal output (host stdout on desktop; USB CDC /dev/ttyACM0 + UART0 on Pico).",
  },
  input: {
    sig: "input(prompt?: str) -> str",
    ir: "%line = call i8* @__omni_input(i8* %prompt_or_null)",
    note: "Target-universal line reader (host stdin on desktop; USB CDC + UART0 getchar() on Pico).",
  },
  getchar: {
    sig: "getchar() -> int",
    ir: "%ch = call i32 @getchar()",
    note: "Blocking single-character console read (libc stdin on desktop; USB CDC + UART0 on Pico).",
  },
  putchar: {
    sig: "putchar(ch: int) -> int",
    ir: "%r = call i32 @putchar(i32 %ch)",
    note: "Single-character console output (libc stdout on desktop; USB CDC + UART0 on Pico).",
  },
  puts: {
    sig: "puts(s: str) -> int",
    ir: "%r = call i32 @puts(i8* %s)",
    note: "Prints null-terminated string + newline.",
  },
  strlen: {
    sig: "strlen(s: str) -> int",
    ir: "%n = call i64 @__omni_str_len(i8* %s)",
    note: "Returns byte length of an OmniScript string.",
  },
  str: {
    sig: "str(n: int | float) -> str",
    ir: "%s = call i8* @__omni_int_to_str(i64 %n)",
    note: "Formats scalar into a heap-allocated null-terminated string.",
  },
  chr: {
    sig: "chr(code: int) -> str",
    ir: "%s = call i8* @__omni_str_chr(i64 %code)",
    note: "Converts ASCII/Unicode codepoint to 1-char string.",
  },
  ord: {
    sig: "ord(ch: str) -> int",
    ir: "%c = call i64 @__omni_str_ord(i8* %ch)",
    note: "Returns integer codepoint of first character.",
  },
  calloc: {
    sig: "calloc(nmemb: int, size: int) -> ptr",
    ir: "%p = call i8* @calloc(i64 %nmemb, i64 %size)",
    note: "Zero-initialized heap allocation (host libc or Cortex-M SRAM bump heap).",
  },
  malloc: {
    sig: "malloc(size: int) -> ptr",
    ir: "%p = call i8* @malloc(i64 %size)",
    note: "Heap allocation (host libc or Cortex-M SRAM bump heap).",
  },
};

// Core polyglot autocomplete idioms (with optional auto-import insertion!)
const POLYGLOT_COMPLETIONS = [
  // C#
  {
    label: "Console.ReadLine()",
    detail: "C# (.NET BCL) -> @__omni_input(i8* null) [LLM Inferred]",
    insertText: "Console.ReadLine()",
    autoImport: 'import "csharp" System;',
    kind: vscode.CompletionItemKind.Method,
  },
  {
    label: "Console.WriteLine",
    detail: "C# (.NET BCL) -> @__omni_print_str / @__omni_print_int",
    insertText: new vscode.SnippetString("Console.WriteLine(${1:msg});"),
    autoImport: 'import "csharp" System;',
    kind: vscode.CompletionItemKind.Method,
  },
  // Java
  {
    label: "System.out.println",
    detail: "Java -> @__omni_print_str / @__omni_print_int",
    insertText: new vscode.SnippetString("System.out.println(${1:msg});"),
    kind: vscode.CompletionItemKind.Method,
  },
  {
    label: "BufferedReader.readLine()",
    detail: "Java (java.io) -> @__omni_input(i8* null) [LLM Inferred]",
    insertText: "BufferedReader.readLine()",
    autoImport: 'import "java" java.io;',
    kind: vscode.CompletionItemKind.Method,
  },
  // Go
  {
    label: "fmt.Println",
    detail: "Go -> @__omni_print_str / @__omni_print_int",
    insertText: new vscode.SnippetString("fmt.Println(${1:msg})"),
    autoImport: 'import "go" fmt;',
    kind: vscode.CompletionItemKind.Function,
  },
  // JavaScript
  {
    label: "console.log",
    detail: "JavaScript -> @__omni_print_str / @__omni_print_int",
    insertText: new vscode.SnippetString("console.log(${1:msg});"),
    kind: vscode.CompletionItemKind.Method,
  },
  {
    label: "prompt()",
    detail: "JavaScript -> @__omni_input(i8* null)",
    insertText: "prompt()",
    kind: vscode.CompletionItemKind.Function,
  },
  // Python
  {
    label: "input",
    detail: "Python / Universal -> call i8* @__omni_input(i8*)",
    insertText: new vscode.SnippetString('input("${1:Prompt: }")'),
    kind: vscode.CompletionItemKind.Function,
  },
  {
    label: "print",
    detail: "Python / Universal -> @__omni_print_str / @__omni_print_int",
    insertText: new vscode.SnippetString("print(${1:val})"),
    kind: vscode.CompletionItemKind.Function,
  },
  // Rust / C / C++
  {
    label: "println!",
    detail: "Rust -> @__omni_print_str / @__omni_print_int",
    insertText: new vscode.SnippetString('println!("${1:msg}")'),
    kind: vscode.CompletionItemKind.Function,
  },
  {
    label: "printf",
    detail: "C / C++ -> call i32 (i8*, ...) @printf",
    insertText: new vscode.SnippetString('printf("${1:%s\\n}", ${2:arg});'),
    kind: vscode.CompletionItemKind.Function,
  },
  {
    label: 'extern "C"',
    detail: "First-class C / HAL ABI extern declaration block",
    insertText: new vscode.SnippetString('extern "C" {\n    fn ${1:sym}(${2:x}: ${3:int}) -> ${4:int};\n}'),
    kind: vscode.CompletionItemKind.Keyword,
  },
  // Polyglot Import Guides (all core ecosystems supported by omnic)
  {
    label: 'import "python" ...',
    detail: "Python ecosystem import (PyPI / oenv / host site-packages)",
    insertText: new vscode.SnippetString('import "python" ${1:module};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "rust" ...',
    detail: "Rust crate import (crates.io / oenv / cargo)",
    insertText: new vscode.SnippetString('import "rust" ${1:crate_name};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "go" ...',
    detail: "Go package import (Go Proxy / oenv / GOPATH)",
    insertText: new vscode.SnippetString('import "go" ${1:pkg};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "node" ...',
    detail: "JavaScript / Node.js package import (npm / oenv)",
    insertText: new vscode.SnippetString('import "node" ${1:package};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "csharp" ...',
    detail: "C# / .NET BCL or NuGet namespace import",
    insertText: new vscode.SnippetString('import "csharp" ${1:System};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "java" ...',
    detail: "Java / JVM class or Maven Central artifact import",
    insertText: new vscode.SnippetString('import "java" ${1:java.io};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "c" ...',
    detail: "C header / native library import (lifted via c_lifter / oenv)",
    insertText: new vscode.SnippetString('import "c" ${1:zlib};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "cpp" ...',
    detail: "C++ header / library import",
    insertText: new vscode.SnippetString('import "cpp" ${1:algorithm};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "lua" ...',
    detail: "Lua / Luarocks module import",
    insertText: new vscode.SnippetString('import "lua" ${1:module};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "zig" ...',
    detail: "Zig module import",
    insertText: new vscode.SnippetString('import "zig" ${1:std};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "./local.omni" as ...',
    detail: "Local OmniScript module import",
    insertText: new vscode.SnippetString('import "./${1:helper}.omni" as ${2:helper};'),
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "pico2w";',
    detail: "Raspberry Pi Pico 2 W (RP2350 + CYW43439 Wi-Fi/TCP/WS) HAL prelude",
    insertText: 'import "pico2w";',
    kind: vscode.CompletionItemKind.Module,
  },
  {
    label: 'import "pico";',
    detail: "Raspberry Pi Pico / Pico W HAL prelude",
    insertText: 'import "pico";',
    kind: vscode.CompletionItemKind.Module,
  },
];

function isExecutable(p) {
  try {
    fs.accessSync(p, fs.constants.X_OK);
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
}

function resolveOmnicBinary(docUri) {
  const cfg = vscode.workspace.getConfiguration("omniscript");
  const configured = (cfg.get("omnicPath") || "").trim();
  const exeName = process.platform === "win32" ? "omnic.exe" : "omnic";

  if (configured && isExecutable(configured)) {
    return configured;
  }

  const candidates = [];
  const folders = vscode.workspace.workspaceFolders || [];
  for (const wf of folders) {
    candidates.push(path.join(wf.uri.fsPath, "target", "release", exeName));
    candidates.push(path.join(wf.uri.fsPath, "target", "debug", exeName));
  }

  if (docUri && docUri.fsPath) {
    let cur = path.dirname(docUri.fsPath);
    for (let i = 0; i < 6; i++) {
      candidates.push(path.join(cur, "target", "release", exeName));
      candidates.push(path.join(cur, "target", "debug", exeName));
      const parent = path.dirname(cur);
      if (parent === cur) break;
      cur = parent;
    }
  }

  for (const c of candidates) {
    if (isExecutable(c)) return c;
  }

  const pathEnv = (process.env.PATH || "").split(path.delimiter);
  for (const dir of pathEnv) {
    if (!dir) continue;
    const p = path.join(dir, exeName);
    if (isExecutable(p)) return p;
  }
  return null;
}

// Parse an OmniScript import line into `{ eco, pkg }` for live package registry lookup
function parseImportLine(lineText) {
  const t = lineText.trim();
  // Explicit ecosystem: `import "python" numpy;` or `import 'rust' serde as s;`
  let m = t.match(/^import\s+["']([A-Za-z0-9_+#.-]+)["']\s+([A-Za-z0-9_./:@-]+)(?:\s+as\s+[A-Za-z0-9_]+)?\s*;?$/);
  if (m) {
    const eco = normalizeEco(m[1]);
    if (eco) return { eco, pkg: m[2] };
  }
  // `from <pkg> import ...` -> Python
  m = t.match(/^from\s+([A-Za-z0-9_.]+)\s+import\s+/);
  if (m) return { eco: "python", pkg: m[1].split(".")[0] };
  // `using <ns>;` -> C#
  m = t.match(/^using\s+([A-Za-z0-9_.]+)\s*;$/);
  if (m && !m[1].startsWith("System")) return { eco: "csharp", pkg: m[1] };
  // `use crate::...` / `use <crate>::...;` -> Rust
  m = t.match(/^use\s+([A-Za-z0-9_]+)::/);
  if (m && m[1] !== "std" && m[1] !== "core" && m[1] !== "crate") {
    return { eco: "rust", pkg: m[1] };
  }
  return null;
}

function normalizeEco(raw) {
  const s = raw.toLowerCase();
  if (s === "python" || s === "py") return "python";
  if (s === "node" || s === "js" || s === "javascript" || s === "npm" || s === "ts") return "node";
  if (s === "rust" || s === "rs" || s === "cargo") return "rust";
  if (s === "java" || s === "jvm" || s === "kotlin") return "java";
  if (s === "csharp" || s === "c#" || s === "cs" || s === "dotnet") return "csharp";
  if (s === "go" || s === "golang") return "go";
  return null;
}

function registryLabel(eco) {
  switch (eco) {
    case "python": return "PyPI";
    case "node": return "npm";
    case "rust": return "crates.io";
    case "java": return "Maven Central";
    case "csharp": return "NuGet";
    case "go": return "Go Proxy";
    default: return eco;
  }
}

function httpsGetJson(url) {
  return new Promise((resolve, reject) => {
    const req = https.get(
      url,
      {
        headers: {
          "User-Agent": "omniscript-vscode/0.2.2",
          Accept: "application/json",
        },
        timeout: 6000,
      },
      (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          httpsGetJson(res.headers.location).then(resolve, reject);
          return;
        }
        let data = "";
        res.on("data", (chunk) => (data += chunk));
        res.on("end", () => {
          if (res.statusCode !== 200) {
            reject(new Error(`HTTP ${res.statusCode}`));
            return;
          }
          try {
            resolve(JSON.parse(data));
          } catch (e) {
            reject(e);
          }
        });
      }
    );
    req.on("error", reject);
    req.on("timeout", () => {
      req.destroy();
      reject(new Error("Request timed out"));
    });
  });
}

function buildFallbackQueries(pkg) {
  const out = [];
  const add = (s) => {
    const t = (s || "").trim();
    if (t && !out.includes(t)) out.push(t);
  };
  add(pkg);
  // Progressively trim dotted / slashed / colon-delimited sub-symbols (e.g. `lombok.AllArgsConstructor` -> `lombok`)
  let cur = pkg;
  while (/[./:]/.test(cur)) {
    cur = cur.replace(/[./:]+[^./:]+$/, "");
    add(cur);
  }
  const parts = pkg.split(/[./:]+/).filter(Boolean);
  if (parts.length > 1) {
    // Also try first meaningful segment (skipping generic `org`/`com`/`net`/`io` prefixes)
    const first = parts[0];
    if (!["org", "com", "net", "io", "java", "javax", "System"].includes(first)) {
      add(first);
    } else if (parts.length >= 2) {
      add(parts[1]);
    }
  }
  return out;
}

async function queryPackageRegistry(eco, pkg) {
  const queries = buildFallbackQueries(pkg);
  if (eco === "python") {
    const results = [];
    const seen = new Set();
    for (const base of queries) {
      for (const cand of [base, `${base}-python`, `py${base}`]) {
        if (seen.has(cand)) continue;
        seen.add(cand);
        try {
          const j = await httpsGetJson(`https://pypi.org/pypi/${encodeURIComponent(cand)}/json`);
          if (j && j.info) {
            results.push({
              name: j.info.name,
              version: j.info.version || "",
              description: (j.info.summary || "").slice(0, 90),
            });
          }
        } catch {
          // ignore misses
        }
        if (results.length >= 3) return results;
      }
      if (results.length > 0) return results;
    }
    return results;
  }
  if (eco === "node") {
    for (const cand of queries) {
      const j = await httpsGetJson(`https://registry.npmjs.org/-/v1/search?text=${encodeURIComponent(cand)}&size=3`);
      const hits = (j.objects || []).slice(0, 3).map((o) => ({
        name: o.package.name,
        version: o.package.version || "",
        description: (o.package.description || "").slice(0, 90),
      }));
      if (hits.length > 0) return hits;
    }
    return [];
  }
  if (eco === "rust") {
    for (const cand of queries) {
      const j = await httpsGetJson(`https://crates.io/api/v1/crates?q=${encodeURIComponent(cand)}&per_page=3`);
      const hits = (j.crates || []).slice(0, 3).map((c) => ({
        name: c.name,
        version: c.max_stable_version || c.max_version || "",
        description: (c.description || "").slice(0, 90),
      }));
      if (hits.length > 0) return hits;
    }
    return [];
  }
  if (eco === "java") {
    for (const cand of queries) {
      const j = await httpsGetJson(`https://search.maven.org/solrsearch/select?q=${encodeURIComponent(cand)}&rows=3&wt=json`);
      const hits = ((j.response && j.response.docs) || []).slice(0, 3).map((d) => ({
        name: `${d.g}:${d.a}`,
        version: d.latestVersion || "",
        description: `Maven Central (${d.p || "jar"})`,
      }));
      if (hits.length > 0) return hits;
    }
    return [];
  }
  if (eco === "csharp") {
    for (const cand of queries) {
      const j = await httpsGetJson(`https://azuresearch-usnc.nuget.org/query?q=${encodeURIComponent(cand)}&take=3`);
      const hits = (j.data || []).slice(0, 3).map((d) => ({
        name: d.id,
        version: d.version || "",
        description: (d.description || "").slice(0, 90),
      }));
      if (hits.length > 0) return hits;
    }
    return [];
  }
  if (eco === "go") {
    for (const cand of queries) {
      try {
        const j = await httpsGetJson(`https://proxy.golang.org/${cand}/@latest`);
        return [{ name: cand, version: j.Version || "", description: "Go Module Proxy" }];
      } catch {
        // try parent path
      }
    }
    return [{ name: pkg, version: "", description: "Direct Go module path" }];
  }
  return [];
}

/**
 * 100%-certain local type resolver: returns the known class/struct name for `varName` ONLY when
 * an unambiguous explicit declaration or direct constructor binding is found in enclosing scope.
 */
function resolveCertainVarType(doc, position, varName, knownTypes) {
  if (!varName || !knownTypes || knownTypes.size === 0) return null;
  const esc = varName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  // 1) Explicit annotation: `let s: Sensor`, `var s: Sensor`, `const s: Sensor`, param `s: Sensor`
  const annotRe = new RegExp(`(?:\\b(?:let|var|const|val)\\s+)?\\b${esc}\\s*:\\s*([A-Z][A-Za-z0-9_]*)\\b`);
  // 2) C#/Java/C++ typed decl: `Sensor s = ...`
  const cStyleRe = new RegExp(`\\b([A-Z][A-Za-z0-9_]*)\\s+${esc}\\s*(?:=|;|\\)|,)`);
  // 3) Direct constructor: `s = Sensor(...)`, `s = new Sensor(...)`, `s = helper.Sensor(...)`, `s = Sensor {`
  const ctorRe = new RegExp(
    `\\b${esc}\\s*(?::=|=)\\s*(?:new\\s+)?(?:[A-Za-z_][A-Za-z0-9_]*\\.)?([A-Z][A-Za-z0-9_]*)\\s*(?:\\(|\\{)`
  );

  for (let ln = position.line; ln >= 0; ln--) {
    const text = doc.lineAt(ln).text;
    for (const re of [annotRe, cStyleRe, ctorRe]) {
      const m = text.match(re);
      if (m && knownTypes.has(m[1])) {
        return m[1];
      }
    }
    // Stop scanning backwards once we pass the enclosing function header
    if (ln < position.line && /^\s*(?:pub\s+)?(?:fn|def|func)\s+/.test(text)) {
      break;
    }
  }
  return null;
}

function resolveRequirementsPath(workspaceDir) {
  const cfg = vscode.workspace.getConfiguration("omniscript");
  const configured = (cfg.get("requirementsFile") || "").trim();
  if (configured) {
    return path.isAbsolute(configured) ? configured : path.join(workspaceDir, configured);
  }
  return path.join(workspaceDir, "omni-requirements.txt");
}

function isPackageInRequirements(reqText, eco, pkg) {
  if (!reqText) return false;
  const candidates = buildFallbackQueries(pkg).map((s) => s.toLowerCase());
  for (const rawLine of reqText.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#") || line.startsWith("//")) continue;
    const low = line.toLowerCase();
    // Parse `'eco' pkg_spec ...` lines (e.g. `'java' org.projectlombok:lombok == 1.18.38`)
    const m = low.match(/^(?:\[git\])?["']([a-z0-9_+#.-]+)["']\s+([^\s=<>!]+)/);
    if (!m) continue;
    const lineEco = normalizeEco(m[1]);
    if (lineEco && lineEco !== eco) continue;
    const spec = m[2]; // e.g. `org.projectlombok:lombok` or `numpy`
    const specLeaf = spec.split(/[:/]/).pop() || spec;
    for (const cand of candidates) {
      if (spec === cand || specLeaf === cand || spec.endsWith(`:${cand}`) || spec.endsWith(`/${cand}`)) {
        return true;
      }
    }
  }
  return false;
}

function appendToOmniRequirements(workspaceDir, eco, pkgName, version) {
  const reqPath = resolveRequirementsPath(workspaceDir);
  const entry = version ? `'${eco}' ${pkgName} == ${version}` : `'${eco}' ${pkgName}`;
  let existing = "";
  if (fs.existsSync(reqPath)) {
    existing = fs.readFileSync(reqPath, "utf8");
    if (existing.includes(pkgName)) {
      return { reqPath, entry, alreadyPresent: true };
    }
  }
  const sep = existing.length > 0 && !existing.endsWith("\n") ? "\n" : "";
  fs.writeFileSync(reqPath, existing + sep + entry + "\n", "utf8");
  return { reqPath, entry, alreadyPresent: false };
}

function activate(context) {
  const collection = vscode.languages.createDiagnosticCollection("omniscript");
  context.subscriptions.push(collection);

  // Per-document cached analysis from `omnic check --json` (`{ platform, diagnostics, symbols }`)
  const analysisCache = new Map();

  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 90);
  statusBar.command = "omniscript.statusBarMenu";
  context.subscriptions.push(statusBar);

  const codeLensEmitter = new vscode.EventEmitter();
  context.subscriptions.push(codeLensEmitter);

  let omniTerminal = null;
  context.subscriptions.push(
    vscode.window.onDidCloseTerminal((t) => {
      if (t === omniTerminal) omniTerminal = null;
    })
  );

  function getOrCreateTerminal(cwd) {
    if (!omniTerminal) {
      omniTerminal = vscode.window.createTerminal({ name: "OmniScript", cwd });
    }
    omniTerminal.show(true);
    return omniTerminal;
  }

  async function runOrBuildActiveFile(mode) {
    const ed = vscode.window.activeTextEditor;
    if (!ed || ed.document.languageId !== "omniscript") {
      vscode.window.showWarningMessage("OmniScript: Open a .omni file to run or build.");
      return;
    }
    const doc = ed.document;
    if (doc.isDirty) {
      await doc.save();
    }
    const omnicBin = resolveOmnicBinary(doc.uri);
    if (!omnicBin) {
      const pick = await vscode.window.showWarningMessage(
        "OmniScript: `omnic` compiler binary not found.",
        "Select omnic Path"
      );
      if (pick === "Select omnic Path") {
        await vscode.commands.executeCommand("omniscript.selectOmnicPath");
      }
      return;
    }

    const filePath = doc.uri.fsPath;
    const wf = vscode.workspace.getWorkspaceFolder(doc.uri);
    const cwd = wf ? wf.uri.fsPath : path.dirname(filePath);
    const cached = analysisCache.get(doc.uri.toString());
    const isPico =
      (cached && cached.platform && /thumb|rp2040|rp2350|pico/i.test(cached.platform)) ||
      /^\s*import\s+["']pico/m.test(doc.getText());
    const stem = filePath.replace(/\.omni$/i, "");
    const term = getOrCreateTerminal(cwd);

    if (mode === "build") {
      const outPath = isPico ? `${stem}.uf2` : stem;
      term.sendText(`"${omnicBin}" build "${filePath}" -o "${outPath}"`);
    } else if (isPico) {
      // Bare-metal Pico: build `.uf2` and flash via hands-free 1200-baud BOOTSEL + picotool!
      const outUf2 = `${stem}.uf2`;
      term.sendText(
        `"${omnicBin}" build "${filePath}" -o "${outUf2}" && ` +
          `(if [ -e /dev/ttyACM0 ]; then stty -F /dev/ttyACM0 115200 && sleep 0.2 && stty -F /dev/ttyACM0 1200 && sleep 1.5; fi) && ` +
          `picotool load -f -v -x "${outUf2}"`
      );
    } else {
      term.sendText(`"${omnicBin}" run "${filePath}"`);
    }
  }

  context.subscriptions.push(
    vscode.commands.registerCommand("omniscript.runFile", () => runOrBuildActiveFile("run")),
    vscode.commands.registerCommand("omniscript.buildFile", () => runOrBuildActiveFile("build"))
  );

  // Command 0: Status bar QuickPick menu (select requirements file, select omnic binary, run install, etc.)
  context.subscriptions.push(
    vscode.commands.registerCommand("omniscript.statusBarMenu", async () => {
      const activeUri = vscode.window.activeTextEditor ? vscode.window.activeTextEditor.document.uri : null;
      const wf =
        (activeUri && vscode.workspace.getWorkspaceFolder(activeUri)) ||
        (vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0]);
      const baseDir = wf ? wf.uri.fsPath : activeUri ? path.dirname(activeUri.fsPath) : process.cwd();
      const curReq = resolveRequirementsPath(baseDir);
      const curBin = resolveOmnicBinary(activeUri) || "not found";

      const choice = await vscode.window.showQuickPick(
        [
          {
            label: "$(play) Run / Flash current .omni file",
            description: "Desktop JIT or Pico 2 W UF2 + picotool",
            action: "runFile",
          },
          {
            label: "$(tools) Build current .omni file",
            description: "Compile native binary or .uf2 + .elf",
            action: "buildFile",
          },
          {
            label: "$(file-text) Select requirements file...",
            description: path.basename(curReq),
            detail: curReq,
            action: "selectReq",
          },
          {
            label: "$(go-to-file) Open active requirements file",
            description: fs.existsSync(curReq) ? "exists" : "will create",
            detail: curReq,
            action: "openReq",
          },
          {
            label: "$(cloud-download) Run omnic install",
            description: path.basename(curReq),
            detail: `"${curBin}" install "${curReq}"`,
            action: "install",
          },
          {
            label: "$(tools) Select omnic binary path...",
            description: curBin === "not found" ? "not configured" : path.basename(curBin),
            detail: curBin,
            action: "selectBin",
          },
          {
            label: "$(refresh) Re-run omnic check",
            description: "Refresh diagnostics, LLVM hovers & spine symbols",
            action: "recheck",
          },
        ],
        { placeHolder: "OmniScript Workspace Actions" }
      );
      if (!choice) return;

      if (choice.action === "runFile") {
        await vscode.commands.executeCommand("omniscript.runFile");
      } else if (choice.action === "buildFile") {
        await vscode.commands.executeCommand("omniscript.buildFile");
      } else if (choice.action === "selectReq") {
        await vscode.commands.executeCommand("omniscript.selectRequirementsFile");
      } else if (choice.action === "openReq") {
        if (!fs.existsSync(curReq)) {
          fs.writeFileSync(curReq, "# OmniScript Polyglot Requirements Manifest\n", "utf8");
        }
        const d = await vscode.workspace.openTextDocument(curReq);
        await vscode.window.showTextDocument(d);
      } else if (choice.action === "install") {
        const omnicBin = resolveOmnicBinary(activeUri) || "omnic";
        const term = vscode.window.createTerminal({ name: "OmniScript Install", cwd: baseDir });
        term.show();
        term.sendText(`"${omnicBin}" install "${curReq}"`);
      } else if (choice.action === "selectBin") {
        await vscode.commands.executeCommand("omniscript.selectOmnicPath");
      } else if (choice.action === "recheck" && vscode.window.activeTextEditor) {
        runCheck(vscode.window.activeTextEditor.document);
      }
    })
  );

  // Command 1b: Select active `omni-requirements.txt` manifest (auto-discovers workspace candidates + browse)
  context.subscriptions.push(
    vscode.commands.registerCommand("omniscript.selectRequirementsFile", async () => {
      const found = await vscode.workspace.findFiles("**/*requirements*.txt", "**/{target,node_modules,venv,.git}/**", 25);
      const picks = found.map((u) => ({
        label: `$(file-text) ${vscode.workspace.asRelativePath(u)}`,
        detail: u.fsPath,
        uri: u,
      }));
      picks.push({
        label: "$(folder-opened) Browse for requirements file...",
        detail: "Choose any manifest file on disk",
        uri: null,
      });

      const chosen = await vscode.window.showQuickPick(picks, {
        placeHolder: "Select active OmniScript requirements manifest",
      });
      if (!chosen) return;

      let selectedPath = chosen.uri ? chosen.uri.fsPath : null;
      if (!selectedPath) {
        const picked = await vscode.window.showOpenDialog({
          canSelectMany: false,
          openLabel: "Use requirements file",
          filters: { "Requirements Manifest": ["txt", "toml", "omni"], "All Files": ["*"] },
        });
        if (!picked || !picked[0]) return;
        selectedPath = picked[0].fsPath;
      }

      const targetScope =
        vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders.length > 0
          ? vscode.ConfigurationTarget.Workspace
          : vscode.ConfigurationTarget.Global;
      await vscode.workspace
        .getConfiguration("omniscript")
        .update("requirementsFile", selectedPath, targetScope);
      codeLensEmitter.fire();
      vscode.window.showInformationMessage(`OmniScript active requirements file: ${selectedPath}`);
      if (vscode.window.activeTextEditor) {
        runCheck(vscode.window.activeTextEditor.document);
      }
    })
  );

  // Command 1: Pick `omnic` binary path
  context.subscriptions.push(
    vscode.commands.registerCommand("omniscript.selectOmnicPath", async () => {
      const picked = await vscode.window.showOpenDialog({
        canSelectMany: false,
        openLabel: "Select omnic binary",
        title: "Locate the OmniScript (omnic) compiler binary",
      });
      if (picked && picked[0]) {
        await vscode.workspace
          .getConfiguration("omniscript")
          .update("omnicPath", picked[0].fsPath, vscode.ConfigurationTarget.Global);
        if (vscode.window.activeTextEditor) {
          runCheck(vscode.window.activeTextEditor.document);
        }
      }
    })
  );

  // Command 2: Live Package Registry Lookup -> append to `omni-requirements.txt`
  context.subscriptions.push(
    vscode.commands.registerCommand("omniscript.lookupImportPackage", async (eco, pkg, docUri) => {
      if (!eco || !pkg) return;
      const regName = registryLabel(eco);
      const matches = await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Notification,
          title: `Searching ${regName} for '${pkg}'...`,
          cancellable: false,
        },
        async () => {
          try {
            return await queryPackageRegistry(eco, pkg);
          } catch (e) {
            vscode.window.showWarningMessage(`OmniScript: ${regName} lookup failed (${e.message}).`);
            return [];
          }
        }
      );

      if (!matches.length) {
        vscode.window.showInformationMessage(`No matches found on ${regName} for '${pkg}'.`);
        return;
      }

      const items = matches.map((m) => ({
        label: m.version ? `${m.name} == ${m.version}` : m.name,
        description: regName,
        detail: m.description,
        pkgData: m,
      }));
      const picked = await vscode.window.showQuickPick(items, {
        placeHolder: `Select ${regName} package to add to omni-requirements.txt`,
      });
      if (!picked) return;

      const wf =
        (docUri && vscode.workspace.getWorkspaceFolder(docUri)) ||
        (vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0]);
      const targetDir = wf
        ? wf.uri.fsPath
        : docUri
        ? path.dirname(docUri.fsPath)
        : process.cwd();

      const res = appendToOmniRequirements(
        targetDir,
        eco,
        picked.pkgData.name,
        picked.pkgData.version
      );
      if (res.alreadyPresent) {
        vscode.window.showInformationMessage(
          `'${picked.pkgData.name}' is already listed in omni-requirements.txt.`
        );
      } else {
        codeLensEmitter.fire();
        const action = await vscode.window.showInformationMessage(
          `Added \`${res.entry}\` to omni-requirements.txt`,
          "Run omnic install",
          "Open File"
        );
        if (action === "Open File") {
          const d = await vscode.workspace.openTextDocument(res.reqPath);
          await vscode.window.showTextDocument(d);
        } else if (action === "Run omnic install") {
          const omnicBin = resolveOmnicBinary(docUri) || "omnic";
          const term = vscode.window.createTerminal({ name: "OmniScript Install", cwd: targetDir });
          term.show();
          term.sendText(`"${omnicBin}" install "${res.reqPath}"`);
        }
      }
    })
  );

  // Provider 1: LLVM IR Hover Provider
  context.subscriptions.push(
    vscode.languages.registerHoverProvider("omniscript", {
      provideHover(doc, position) {
        const wordRange = doc.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_.]*/);
        const simpleRange = doc.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/);
        const dotted = wordRange ? doc.getText(wordRange) : "";
        const word = simpleRange ? doc.getText(simpleRange) : "";
        const cached = analysisCache.get(doc.uri.toString());

        // 1. Check if this line is an LLM-inferred site (`OMNI_LLM_INFERRED`)
        if (cached && cached.diagnostics) {
          const line1 = position.line + 1;
          const llmDiag = cached.diagnostics.find(
            (d) => d.code === "OMNI_LLM_INFERRED" && d.line === line1
          );
          if (llmDiag) {
            const md = new vscode.MarkdownString();
            md.appendMarkdown(`**OmniScript LLM-Inferred Lowering** (\`${llmDiag.ecosystem || "polyglot"}\`)\n\n`);
            const intent = llmDiag.intent || dotted || word;
            let previewIr = `; Synthesized at compile time by models/omni-coder.gguf\n; Intent: ${intent}`;
            if (/readline|readstring|gets|prompt/i.test(intent)) {
              previewIr = `; Anticipated LLVM IR (via omni-coder.gguf)\n%t = call i8* @__omni_input(i8* null)\n%p = ptrtoint i8* %t to i64`;
            } else if (/writeline|println|console\.log/i.test(intent)) {
              previewIr = `; Anticipated LLVM IR (via omni-coder.gguf)\ncall void @__omni_print_str(i8* %arg)`;
            } else if (/\.read\(\)/i.test(intent)) {
              previewIr = `; Anticipated LLVM IR (via omni-coder.gguf)\n%c = call i32 @getchar()\n%c64 = sext i32 %c to i64`;
            }
            md.appendCodeblock(previewIr, "llvm");
            return new vscode.Hover(md, wordRange || simpleRange);
          }
        }

        // 2. Check exact observed symbols from `omnic check --json` (user functions, structs/classes, HAL externs, runtime decls)
        if (cached && cached.symbols && word) {
          const sym = cached.symbols.find((s) => s.name === word);
          if (sym) {
            const md = new vscode.MarkdownString();
            const label =
              sym.kind === "hal_extern"
                ? "Bare-Metal HAL `extern fn`"
                : sym.kind === "struct"
                ? "OmniScript Struct / Class"
                : sym.kind === "function"
                ? "OmniScript Function"
                : sym.kind === "runtime"
                ? "OmniScript Runtime Intrinsic"
                : "External C / FFI Symbol";
            md.appendMarkdown(`**${label}**: \`${sym.signature || sym.name}\`\n\n`);
            if (sym.doc) {
              md.appendMarkdown(`${sym.doc}\n\n`);
            }
            if (sym.usage) {
              md.appendMarkdown(`**Usage:**\n`);
              md.appendCodeblock(sym.usage, "omniscript");
            }
            md.appendMarkdown(`**Lowered LLVM IR Symbol:**\n`);
            md.appendCodeblock(sym.llvm_ir, "llvm");
            if (sym.file && sym.line) {
              md.appendMarkdown(`\nDefined at \`${path.basename(sym.file)}:${sym.line}\` *(Ctrl+Click to jump)*`);
            }
            return new vscode.Hover(md, simpleRange);
          }
        }

        // 2a. Check 100%-certain `<var>.<member>` class/struct field or method hover
        if (cached && cached.types && dotted.includes(".")) {
          const parts = dotted.split(".");
          if (parts.length === 2) {
            const [recv, member] = parts;
            const typeMap = new Map(cached.types.map((t) => [t.name, t]));
            const certainType = resolveCertainVarType(doc, position, recv, typeMap);
            if (certainType) {
              const tInfo = typeMap.get(certainType);
              const fld = (tInfo.fields || []).find((f) => f.name === member);
              if (fld) {
                const md = new vscode.MarkdownString();
                md.appendMarkdown(`**Field** \`${certainType}.${fld.name}: ${fld.type}\`\n\n`);
                md.appendCodeblock(`getelementptr inbounds %struct.${certainType}, %struct.${certainType}* %${recv}, i32 0, i32 <idx>`, "llvm");
                return new vscode.Hover(md, wordRange);
              }
              const meth = (tInfo.methods || []).find((m) => m.name === member);
              if (meth) {
                const md = new vscode.MarkdownString();
                md.appendMarkdown(`**Method** \`${certainType}.${meth.signature}\`\n\n`);
                if (meth.doc) {
                  md.appendMarkdown(`${meth.doc}\n\n`);
                }
                if (meth.usage) {
                  md.appendMarkdown(`**Usage:**\n`);
                  md.appendCodeblock(meth.usage, "omniscript");
                }
                md.appendCodeblock(meth.llvm_ir, "llvm");
                if (tInfo.file && meth.line) {
                  md.appendMarkdown(`\nDefined at \`${path.basename(tInfo.file)}:${meth.line}\` *(Ctrl+Click to jump)*`);
                }
                return new vscode.Hover(md, wordRange);
              }
            }
          }
        }

        // 2b. Check 1-2 hop dependency spine symbols (`varint.encode`, `helper.Calc.add`, etc.)
        if (cached && cached.spine_symbols) {
          const sp =
            cached.spine_symbols.find((s) => s.qualified === dotted) ||
            cached.spine_symbols.find((s) => s.name === word);
          if (sp) {
            const md = new vscode.MarkdownString();
            const hopLabel = sp.hop === 2 ? "2-Hop Method" : sp.kind === "spine_class" ? "Class / Struct" : "Function";
            md.appendMarkdown(
              `**Drilled Spine ${hopLabel}** (\`${sp.ecosystem}\` · \`${sp.module}\`): \`${sp.qualified}\`\n\n`
            );
            if (sp.doc) {
              md.appendMarkdown(`${sp.doc}\n\n`);
            }
            md.appendCodeblock(sp.signature || sp.qualified, "text");
            if (sp.usage) {
              md.appendMarkdown(`**Usage:**\n`);
              md.appendCodeblock(sp.usage, "omniscript");
            }
            if (sp.file && sp.line) {
              md.appendMarkdown(`\nSource: \`${path.basename(sp.file)}:${sp.line}\` *(Ctrl+Click to jump)*`);
            }
            return new vscode.Hover(md, wordRange || simpleRange);
          }
        }

        // 3. Fallback to universal builtin LLVM IR catalog (`print`, `input`, `strlen`, `getchar`, etc.)
        if (word && BUILTIN_LLVM_HOVERS[word]) {
          const info = BUILTIN_LLVM_HOVERS[word];
          const md = new vscode.MarkdownString();
          md.appendMarkdown(`**OmniScript Universal Builtin**: \`${info.sig}\`\n\n`);
          md.appendCodeblock(info.ir, "llvm");
          md.appendMarkdown(`\n${info.note}`);
          return new vscode.Hover(md, simpleRange);
        }

        return null;
      },
    })
  );

  // Provider 1b: Go-to-Definition Provider (user functions, structs/classes, local .omni, & 1-2 hop spine files)
  context.subscriptions.push(
    vscode.languages.registerDefinitionProvider("omniscript", {
      provideDefinition(doc, position) {
        const cached = analysisCache.get(doc.uri.toString());
        if (!cached) return null;
        const wordRange = doc.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_.]*/);
        const simpleRange = doc.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/);
        const dotted = wordRange ? doc.getText(wordRange) : "";
        const word = simpleRange ? doc.getText(simpleRange) : "";

        // 1. 100%-certain `<var>.<member>` class/struct method or field Go-to-Definition
        if (cached.types && dotted.includes(".")) {
          const [recv, member] = dotted.split(".", 2);
          const typeMap = new Map(cached.types.map((t) => [t.name, t]));
          const certainType = resolveCertainVarType(doc, position, recv, typeMap);
          if (certainType) {
            const tInfo = typeMap.get(certainType);
            if (tInfo && tInfo.file && fs.existsSync(tInfo.file)) {
              const meth = (tInfo.methods || []).find((m) => m.name === member);
              const targetLine = meth && meth.line ? meth.line : tInfo.line || 1;
              return new vscode.Location(
                vscode.Uri.file(tInfo.file),
                new vscode.Position(Math.max(0, targetLine - 1), 0)
              );
            }
          }
        }

        // 2. User functions & structs/classes in current file
        if (cached.symbols && word) {
          const sym = cached.symbols.find((s) => s.name === word && s.file && s.line);
          if (sym && fs.existsSync(sym.file)) {
            const pos = new vscode.Position(Math.max(0, sym.line - 1), 0);
            return new vscode.Location(vscode.Uri.file(sym.file), pos);
          }
        }
        // 3. Drilled 1-2 hop dependency spine symbols & local `.omni` imports (exact qualified match first)
        if (cached.spine_symbols) {
          const sp =
            cached.spine_symbols.find((s) => s.qualified === dotted && s.file && s.line) ||
            cached.spine_symbols.find((s) => s.name === word && s.file && s.line);
          if (sp && fs.existsSync(sp.file)) {
            const pos = new vscode.Position(Math.max(0, sp.line - 1), 0);
            return new vscode.Location(vscode.Uri.file(sp.file), pos);
          }
        }
        return null;
      },
    })
  );

  // Provider 2: Polyglot IntelliSense + Auto-Import + Dependency Spine Completion Provider
  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      "omniscript",
      {
        provideCompletionItems(doc, position) {
          const fullText = doc.getText();
          const linePrefix = doc.lineAt(position).text.slice(0, position.character);
          const dotMatch = linePrefix.match(
            /([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)?)\.$/
          );
          const cached = analysisCache.get(doc.uri.toString());
          const items = [];

          // If user just typed `<ident>.` or `<mod>.<Class>.` (max 2 hops), check:
          // (a) 100%-certain class/struct instance members, then (b) 1-2 hop spine exports!
          if (dotMatch && cached) {
            const chain = dotMatch[1];
            if (!chain.includes(".") && cached.types && cached.types.length > 0) {
              const typeMap = new Map(cached.types.map((t) => [t.name, t]));
              const certainType = resolveCertainVarType(doc, position, chain, typeMap);
              if (certainType) {
                const tInfo = typeMap.get(certainType);
                for (const f of tInfo.fields || []) {
                  const ci = new vscode.CompletionItem(f.name, vscode.CompletionItemKind.Field);
                  ci.detail = `${certainType}.${f.name}: ${f.type}`;
                  items.push(ci);
                }
                for (const m of tInfo.methods || []) {
                  const ci = new vscode.CompletionItem(m.name, vscode.CompletionItemKind.Method);
                  ci.detail = `${certainType}.${m.signature}`;
                  const md = new vscode.MarkdownString();
                  if (m.doc) md.appendMarkdown(`${m.doc}\n\n`);
                  if (m.usage) md.appendCodeblock(m.usage, "omniscript");
                  if (m.llvm_ir) md.appendCodeblock(m.llvm_ir, "llvm");
                  ci.documentation = md;
                  ci.insertText = new vscode.SnippetString(`${m.name}($0)`);
                  items.push(ci);
                }
                if (items.length > 0) {
                  return items;
                }
              }
            }
            if (cached.spine_symbols) {
              const aliasPrefix = chain + ".";
              for (const sp of cached.spine_symbols) {
                if (!sp.qualified || !sp.qualified.startsWith(aliasPrefix)) continue;
                const remainder = sp.qualified.slice(aliasPrefix.length);
                // Only show the immediate next hop (Hop 1 on `mod.`, Hop 2 on `mod.Class.`)
                if (!remainder || remainder.includes(".")) continue;
                const kind =
                  sp.kind === "spine_class"
                    ? vscode.CompletionItemKind.Class
                    : sp.kind === "spine_method"
                    ? vscode.CompletionItemKind.Method
                    : vscode.CompletionItemKind.Function;
                const ci = new vscode.CompletionItem(sp.name, kind);
                ci.detail = `[${sp.ecosystem} spine] ${sp.signature || sp.qualified}`;
                const md = new vscode.MarkdownString();
                if (sp.doc) {
                  md.appendMarkdown(`${sp.doc}\n\n`);
                }
                if (sp.usage) {
                  md.appendCodeblock(sp.usage, "omniscript");
                }
                md.appendMarkdown(`Drilled from \`${path.basename(sp.file || "")}:${sp.line || 1}\``);
                ci.documentation = md;
                if (sp.kind === "spine_fn" || sp.kind === "spine_method") {
                  ci.insertText = new vscode.SnippetString(`${sp.name}($0)`);
                }
                items.push(ci);
              }
              if (items.length > 0) {
                return items;
              }
            }
          }

          // 1. Curated core polyglot idioms & import guides with optional auto-import
          for (const spec of POLYGLOT_COMPLETIONS) {
            const ci = new vscode.CompletionItem(spec.label, spec.kind);
            ci.detail = spec.detail;
            ci.insertText = spec.insertText;
            if (spec.autoImport && !fullText.includes(spec.autoImport)) {
              ci.additionalTextEdits = [
                vscode.TextEdit.insert(new vscode.Position(0, 0), `${spec.autoImport}\n`),
              ];
              ci.documentation = new vscode.MarkdownString(
                `Auto-adds \`${spec.autoImport}\` at top of file.`
              );
            }
            items.push(ci);
          }

          // 2. Dynamic symbols from `omnic check --json` (in-scope user fns, structs/classes, & HAL externs)
          if (cached && cached.symbols) {
            const seen = new Set(POLYGLOT_COMPLETIONS.map((p) => p.label));
            for (const sym of cached.symbols) {
              if (sym.kind === "runtime" || seen.has(sym.name)) continue;
              seen.add(sym.name);
              const kind =
                sym.kind === "hal_extern"
                  ? vscode.CompletionItemKind.Interface
                  : sym.kind === "struct"
                  ? vscode.CompletionItemKind.Struct
                  : vscode.CompletionItemKind.Function;
              const ci = new vscode.CompletionItem(sym.name, kind);
              ci.detail = sym.signature || sym.llvm_ir;
              const md = new vscode.MarkdownString();
              if (sym.doc) md.appendMarkdown(`${sym.doc}\n\n`);
              if (sym.usage) md.appendCodeblock(sym.usage, "omniscript");
              if (sym.llvm_ir) md.appendCodeblock(sym.llvm_ir, "llvm");
              ci.documentation = md;
              items.push(ci);
            }
          }

          // 3. Drilled Hop-1 dependency spine qualified symbols (e.g. `varint.encode`, `helper.Calculator`)
          // (Hop-2 methods appear cleanly on `mod.Class.` dot-walk to prevent top-level bloat)
          if (cached && cached.spine_symbols) {
            for (const sp of cached.spine_symbols) {
              if (sp.hop && sp.hop > 1) continue;
              const kind =
                sp.kind === "spine_class"
                  ? vscode.CompletionItemKind.Class
                  : vscode.CompletionItemKind.Function;
              const ci = new vscode.CompletionItem(sp.qualified, kind);
              ci.detail = `[${sp.ecosystem} spine] ${sp.signature || sp.qualified}`;
              const md = new vscode.MarkdownString();
              if (sp.doc) md.appendMarkdown(`${sp.doc}\n\n`);
              if (sp.usage) md.appendCodeblock(sp.usage, "omniscript");
              md.appendMarkdown(`Drilled from \`${path.basename(sp.file || "")}:${sp.line || 1}\``);
              ci.documentation = md;
              if (sp.kind === "spine_fn") {
                ci.insertText = new vscode.SnippetString(`${sp.qualified}($0)`);
              }
              items.push(ci);
            }
          }
          return items;
        },
      },
      ".",
      '"'
    )
  );

  // Provider 3: Import Line CodeLens ("Lookup Package & Add to omni-requirements.txt")
  // Automatically hides any import whose package (or fallback root) is already in the active requirements file!
  context.subscriptions.push(
    vscode.languages.registerCodeLensProvider("omniscript", {
      onDidChangeCodeLenses: codeLensEmitter.event,
      provideCodeLenses(doc) {
        const wf =
          vscode.workspace.getWorkspaceFolder(doc.uri) ||
          (vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0]);
        const baseDir = wf ? wf.uri.fsPath : path.dirname(doc.uri.fsPath);
        const reqPath = resolveRequirementsPath(baseDir);
        let reqText = "";
        try {
          if (fs.existsSync(reqPath)) {
            reqText = fs.readFileSync(reqPath, "utf8");
          }
        } catch {
          reqText = "";
        }

        const lenses = [];
        for (let i = 0; i < doc.lineCount; i++) {
          const parsed = parseImportLine(doc.lineAt(i).text);
          if (!parsed) continue;
          if (isPackageInRequirements(reqText, parsed.eco, parsed.pkg)) {
            continue;
          }
          const range = new vscode.Range(i, 0, i, 0);
          const regName = registryLabel(parsed.eco);
          lenses.push(
            new vscode.CodeLens(range, {
              title: `$(search) Lookup '${parsed.pkg}' on ${regName} -> add to ${path.basename(reqPath)}`,
              command: "omniscript.lookupImportPackage",
              arguments: [parsed.eco, parsed.pkg, doc.uri],
            })
          );
        }
        return lenses;
      },
    })
  );

  function runCheck(doc) {
    if (!doc || doc.languageId !== "omniscript" || doc.uri.scheme !== "file") {
      return;
    }
    const cfg = vscode.workspace.getConfiguration("omniscript");
    if (!cfg.get("enableDiagnostics", true)) {
      collection.delete(doc.uri);
      statusBar.hide();
      return;
    }

    const omnicBin = resolveOmnicBinary(doc.uri);
    if (!omnicBin) {
      collection.delete(doc.uri);
      statusBar.text = "$(warning) OmniScript: set omnicPath";
      statusBar.tooltip = "omnic binary not found in workspace or PATH. Click to select.";
      statusBar.show();
      return;
    }

    const showHints = cfg.get("showLlmInferredHints", true);
    const cwd = path.dirname(doc.uri.fsPath);

    cp.execFile(
      omnicBin,
      ["check", "--json", doc.uri.fsPath],
      { cwd, timeout: 10000, maxBuffer: 2 * 1024 * 1024 },
      (_err, stdout) => {
        if (!stdout || !stdout.trim()) return;
        let parsed;
        try {
          parsed = JSON.parse(stdout.trim());
        } catch {
          return;
        }
        analysisCache.set(doc.uri.toString(), parsed);

        const diags = [];
        for (const item of parsed.diagnostics || []) {
          const isError = item.severity === "error";
          if (!isError && !showHints) continue;
          const lineIdx = Math.max(0, (item.line || 1) - 1);
          const safeLine = Math.min(lineIdx, Math.max(0, doc.lineCount - 1));
          const lineText = doc.lineAt(safeLine).text;
          const colIdx = Math.max(0, Math.min((item.col || 1) - 1, lineText.length));
          const range = new vscode.Range(
            safeLine,
            colIdx,
            safeLine,
            Math.max(colIdx + 1, lineText.length)
          );
          const severity = isError
            ? vscode.DiagnosticSeverity.Error
            : vscode.DiagnosticSeverity.Information;
          const diag = new vscode.Diagnostic(range, item.message || "", severity);
          diag.source = "omnic";
          diag.code = item.code || (isError ? "OMNI_ERROR" : "OMNI_LLM_INFERRED");
          diags.push(diag);
        }

        collection.set(doc.uri, diags);
        const llmCount = diags.filter((d) => d.code === "OMNI_LLM_INFERRED").length;
        statusBar.text = `$(chip) omnic: ${parsed.platform || "ready"}${llmCount > 0 ? ` (${llmCount} LLM)` : ""}`;
        statusBar.tooltip = `Using ${omnicBin}\nClick to change omnic binary path`;
        statusBar.show();
      }
    );
  }

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(runCheck),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.fileName && doc.fileName.toLowerCase().includes("requirements")) {
        codeLensEmitter.fire();
      }
      runCheck(doc);
    }),
    vscode.window.onDidChangeActiveTextEditor((ed) => {
      if (ed && ed.document.languageId === "omniscript") {
        runCheck(ed.document);
      } else {
        statusBar.hide();
      }
    }),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("omniscript") && vscode.window.activeTextEditor) {
        runCheck(vscode.window.activeTextEditor.document);
      }
    })
  );

  if (vscode.window.activeTextEditor) {
    runCheck(vscode.window.activeTextEditor.document);
  }
}

function deactivate() {}

module.exports = { activate, deactivate };
