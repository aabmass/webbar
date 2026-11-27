#!/usr/bin/env node
/**
 * Validates that every CSS custom property referenced via `var(--name)` across
 * the reference bar stylesheets and Svelte component `<style>` blocks is either
 * declared in CSS or provided by the runtime host/component allowlist.
 */
import fs from "node:fs";
import path from "node:path";

const ROOT = process.cwd();
const SCAN_DIRS = [
  "examples/sway-full-bar-shared/styles",
  "examples/sway-full-bar-svelte/src",
  "examples/sway-greeter-svelte/src",
];

const RUNTIME_PREFIXES = [
  "--desktop-",
  "--gtk-",
  "--webbar-",
  "--toast-",
  "--popover-",
];

const INLINE_VARS = new Set(["--caret-right", "--icon-src", "--fill"]);

function walk(dir, out = []) {
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(full, out);
    } else if (entry.name.endsWith(".css") || entry.name.endsWith(".svelte")) {
      out.push(full);
    }
  }
  return out;
}

function extractCss(filePath, content) {
  if (filePath.endsWith(".css")) {
    return [{ css: content, lineOffset: 0 }];
  }
  const blocks = [];
  const re = /<style\b[^>]*>([\s\S]*?)<\/style>/g;
  let match;
  while ((match = re.exec(content)) !== null) {
    const before = content.slice(0, match.index);
    const lineOffset = before.split("\n").length - 1;
    blocks.push({ css: match[1], lineOffset });
  }
  return blocks;
}

const files = SCAN_DIRS.flatMap((d) => walk(path.join(ROOT, d)));
if (files.length === 0) {
  console.error("check-css-vars: no CSS or Svelte files found to scan");
  process.exit(1);
}
const defined = new Set();
const usages = [];

const declRe = /(--[a-zA-Z0-9_-]+)\s*:/g;
const varRe = /var\(\s*(--[a-zA-Z0-9_-]+)/g;

for (const file of files) {
  const raw = fs.readFileSync(file, "utf8");
  for (const { css, lineOffset } of extractCss(file, raw)) {
    const cleaned = css.replace(/\/\*[\s\S]*?\*\//g, (m) =>
      m.replace(/[^\n]/g, " "),
    );
    for (const m of cleaned.matchAll(declRe)) {
      defined.add(m[1]);
    }
    const lines = cleaned.split("\n");
    lines.forEach((line, idx) => {
      for (const m of line.matchAll(varRe)) {
        usages.push({
          file: path.relative(ROOT, file),
          line: lineOffset + idx + 1,
          name: m[1],
        });
      }
    });
  }
}

const errors = usages.filter(
  ({ name }) =>
    !defined.has(name) &&
    !INLINE_VARS.has(name) &&
    !RUNTIME_PREFIXES.some((p) => name.startsWith(p)),
);

if (errors.length > 0) {
  console.error("Undefined CSS custom properties found:");
  for (const err of errors) {
    console.error(`  ${err.file}:${err.line}: var(${err.name}) is not defined`);
  }
  process.exit(1);
}

console.log(
  `check-css-vars: OK (${defined.size} defined tokens, ${usages.length} var() usages across ${files.length} files)`,
);
