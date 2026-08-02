import { existsSync, readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createScanner, SyntaxKind } from "typescript/unstable/ast";
import { describe, expect, it } from "vitest";

const adapterDir = path.dirname(fileURLToPath(import.meta.url));
const sourceRoot = path.resolve(adapterDir, "../..");
const adapterEntries = [
  "desktop-adapter.ts",
  "empty-desktop-adapter.ts",
  "mock-desktop-adapter.ts",
  "DesktopAdapterContext.tsx",
].map((file) => path.join(adapterDir, file));

type Mode = "adapter" | "consumer";
type Token = { readonly kind: SyntaxKind; readonly text: string; readonly value: string };
type ModuleRef = { readonly kind: string; readonly specifier: string | null };

function scan(source: string): Token[] {
  const scanner = createScanner(true, undefined, source);
  const result: Token[] = [];
  for (let kind = scanner.scan(); kind !== SyntaxKind.EndOfFile; kind = scanner.scan()) {
    result.push({ kind, text: scanner.getTokenText(), value: scanner.getTokenValue() });
  }
  return result;
}

function literal(token: Token | undefined) {
  return token?.kind === SyntaxKind.StringLiteral ? token.value : null;
}

function afterFrom(tokens: readonly Token[], start: number) {
  for (let index = start; index < tokens.length; index += 1) {
    const kind = tokens[index].kind;
    if (kind === SyntaxKind.SemicolonToken) return null;
    if (
      index > start &&
      (kind === SyntaxKind.ImportKeyword || kind === SyntaxKind.ExportKeyword)
    ) {
      return null;
    }
    if (kind === SyntaxKind.FromKeyword) return literal(tokens[index + 1]);
  }
  return null;
}

function moduleRefs(tokens: readonly Token[]): ModuleRef[] {
  const refs: ModuleRef[] = [];
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const next = tokens[index + 1];
    if (token.kind === SyntaxKind.ImportKeyword) {
      if (next?.kind === SyntaxKind.OpenParenToken) {
        refs.push({ kind: "dynamic import", specifier: literal(tokens[index + 2]) });
      } else if (next?.kind !== SyntaxKind.DotToken) {
        refs.push({
          kind: "static import",
          specifier: literal(next) ?? afterFrom(tokens, index + 1),
        });
      }
    } else if (token.kind === SyntaxKind.ExportKeyword) {
      const specifier = afterFrom(tokens, index + 1);
      if (specifier !== null) refs.push({ kind: "export-from", specifier });
    }
    if (token.text === "require" && next?.kind === SyntaxKind.OpenParenToken) {
      refs.push({ kind: "require", specifier: literal(tokens[index + 2]) });
    }
  }
  return refs;
}

function segment(tokens: readonly Token[], index: number) {
  if (tokens[index]?.kind === SyntaxKind.Identifier) {
    return { name: tokens[index].value, next: index - 1, computed: false };
  }
  if (
    tokens[index]?.kind === SyntaxKind.CloseBracketToken &&
    tokens[index - 2]?.kind === SyntaxKind.OpenBracketToken
  ) {
    const name = literal(tokens[index - 1]);
    if (name !== null) return { name, next: index - 3, computed: true };
  }
  return null;
}

function callee(tokens: readonly Token[], openParen: number): string[] {
  const names: string[] = [];
  let part = segment(tokens, openParen - 1);
  if (part === null) return names;
  names.unshift(part.name);
  let cursor = part.next;
  if (part.computed && (part = segment(tokens, cursor)) !== null) {
    names.unshift(part.name);
    cursor = part.next;
  }
  while (
    tokens[cursor]?.kind === SyntaxKind.DotToken ||
    tokens[cursor]?.kind === SyntaxKind.QuestionDotToken
  ) {
    part = segment(tokens, cursor - 1);
    if (part === null) break;
    names.unshift(part.name);
    cursor = part.next;
  }
  return names;
}

function deniedCall(names: readonly string[], mode: Mode) {
  const last = names.at(-1);
  if (last === "invoke" || last === "listen") return names.join(".");
  if (mode === "consumer") return null;
  const dotted = names.join(".");
  if (
    ["fetch", "WebSocket", "XMLHttpRequest", "EventSource", "Date"].includes(last ?? "") ||
    names.at(-2) === "Date" ||
    dotted.endsWith("performance.now") ||
    dotted.endsWith("Math.random") ||
    dotted.endsWith("crypto.randomUUID") ||
    dotted.endsWith("crypto.getRandomValues")
  ) {
    return dotted;
  }
  return null;
}

function analyze(source: string, filename: string, mode: Mode) {
  const tokens = scan(source);
  const refs = moduleRefs(tokens);
  const issues: string[] = [];
  for (const ref of refs) {
    if (ref.specifier === null) {
      issues.push(`${filename}: ${ref.kind} must use a string literal`);
    } else if (!ref.specifier.startsWith(".")) {
      const tauri = ref.specifier === "@tauri-apps" || ref.specifier.startsWith("@tauri-apps/");
      if (tauri || (mode === "adapter" && ref.specifier !== "react")) {
        issues.push(`${filename}: forbidden ${ref.kind} ${ref.specifier}`);
      }
    }
  }
  tokens.forEach((token, index) => {
    if (token.kind !== SyntaxKind.OpenParenToken) return;
    const denied = deniedCall(callee(tokens, index), mode);
    if (denied !== null) issues.push(`${filename}: forbidden call ${denied}`);
  });
  return { issues, refs };
}

function excluded(file: string) {
  const relative = path.relative(sourceRoot, file).split(path.sep).join("/");
  return (
    relative === "main.tsx" ||
    relative.startsWith("generated/") ||
    relative.startsWith("test/") ||
    relative.includes(".test.") ||
    relative.includes(".spec.")
  );
}

function resolveImport(from: string, specifier: string) {
  const base = path.resolve(path.dirname(from), specifier);
  const candidates = base.endsWith(".ts") || base.endsWith(".tsx")
    ? [base]
    : [`${base}.ts`, `${base}.tsx`, path.join(base, "index.ts"), path.join(base, "index.tsx")];
  return candidates.find((candidate) => existsSync(candidate)) ?? null;
}

function graph(entries: readonly string[], mode: Mode) {
  const pending = [...entries];
  const visited = new Set<string>();
  const issues: string[] = [];
  while (pending.length > 0) {
    const file = path.resolve(pending.pop()!);
    if (visited.has(file) || excluded(file)) continue;
    visited.add(file);
    const result = analyze(readFileSync(file, "utf8"), file, mode);
    issues.push(...result.issues);
    result.refs.forEach((ref) => {
      if (!ref.specifier?.startsWith(".")) return;
      const resolved = resolveImport(file, ref.specifier);
      if (resolved !== null) pending.push(resolved);
    });
  }
  return issues;
}

function productionFiles(directory: string): string[] {
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) return productionFiles(file);
    return (file.endsWith(".ts") || file.endsWith(".tsx")) && !excluded(file) ? [file] : [];
  });
}

describe("DesktopAdapter source boundary", () => {
  it("keeps the adapter graph isolated", () => {
    expect(graph(adapterEntries, "adapter")).toEqual([]);
  });

  it("prevents consumers from bypassing the adapter", () => {
    const entries = ["app", "ui", "features"].flatMap((dir) =>
      productionFiles(path.join(sourceRoot, dir)),
    );
    expect(graph(entries, "consumer")).toEqual([]);
  });

  it.each([
    ["dynamic import", 'void import("@tauri-apps/api/core")'],
    ["side-effect import", 'import "@tauri-apps/api"'],
    ["export-from", 'export * from "@tauri-apps/api/core"'],
    ["require", 'require("@tauri-apps/api/core")'],
    ["fetch", 'fetch /* gap */ ("fixture")'],
    ["Date", "new Date /* gap */ ()"],
    ["performance", "performance . now ()"],
    ["random", "Math . random ()"],
    ["randomUUID", "crypto . randomUUID ()"],
    ["getRandomValues", "crypto.getRandomValues (buffer)"],
    ["WebSocket", 'new WebSocket ("fixture")'],
    ["XMLHttpRequest", "new XMLHttpRequest ()"],
    ["EventSource", 'new EventSource ("fixture")'],
    ["invoke", 'bridge . invoke ("fixture")'],
    ["listen", 'bridge . listen ("fixture")'],
  ])("detects adversarial %s syntax", (_label, source) => {
    expect(analyze(source, "adversarial.ts", "adapter").issues).not.toEqual([]);
  });
});
