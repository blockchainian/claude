// ABOUTME: Tiny argparse-compatible option parser shared by the two heap-snapshot scripts.
// ABOUTME: Reproduces argparse's usage/error text and exit codes (0 for --help, 2 for bad arguments).
import { basename } from "node:path";

// spec: { description, options: [{ flag, dest, type: "str"|"int"|"float"|"path"|"flag", default, required, help, metavar }] }
export function parseArgs(argv, spec, prog = basename(process.argv[1] ?? "")) {
  const byFlag = new Map(spec.options.map((o) => [o.flag, o]));
  const usageParts = spec.options.map((o) => {
    const text = o.type === "flag" ? o.flag : `${o.flag} ${o.metavar ?? o.dest.toUpperCase()}`;
    return o.required ? text : `[${text}]`;
  });
  const usage = `usage: ${prog} [-h] ${usageParts.join(" ")}`;
  const fail = (msg) => {
    process.stderr.write(`${usage}\n${prog}: error: ${msg}\n`);
    process.exit(2);
  };

  const out = {};
  for (const o of spec.options) out[o.dest] = o.type === "flag" ? false : (o.default ?? null);
  const seen = new Set();
  const extra = [];
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "-h" || arg === "--help") {
      const lines = [usage, "", spec.description, "", "options:", "  -h, --help            show this help message and exit"];
      for (const o of spec.options) {
        const head = o.type === "flag" ? o.flag : `${o.flag} ${o.metavar ?? o.dest.toUpperCase()}`;
        lines.push(head.length < 22 ? `  ${head.padEnd(22)}${o.help}` : `  ${head}\n                        ${o.help}`);
      }
      process.stdout.write(lines.join("\n") + "\n");
      process.exit(0);
    }
    let flag = arg;
    let value = null;
    const eq = arg.indexOf("=");
    if (arg.startsWith("--") && eq > 0) {
      flag = arg.slice(0, eq);
      value = arg.slice(eq + 1);
    }
    const o = byFlag.get(flag);
    if (!o) {
      extra.push(arg);
      continue;
    }
    seen.add(o.dest);
    if (o.type === "flag") {
      out[o.dest] = true;
      continue;
    }
    if (value === null) {
      if (i + 1 >= argv.length || (argv[i + 1].startsWith("-") && argv[i + 1].length > 1 && Number.isNaN(Number(argv[i + 1])))) {
        fail(`argument ${o.flag}: expected one argument`);
      }
      value = argv[++i];
    }
    out[o.dest] = convert(o, value, fail);
  }
  if (extra.length) fail(`unrecognized arguments: ${extra.join(" ")}`);
  const missing = spec.options.filter((o) => o.required && !seen.has(o.dest)).map((o) => o.flag);
  if (missing.length) fail(`the following arguments are required: ${missing.join(", ")}`);
  return out;
}

function convert(o, value, fail) {
  if (o.type === "int") {
    if (!/^\s*[+-]?\d+\s*$/.test(value)) fail(`argument ${o.flag}: invalid int value: ${pyRepr(value)}`);
    return Number.parseInt(value, 10);
  }
  if (o.type === "float") {
    const n = Number(value.trim());
    if (value.trim() === "" || Number.isNaN(n)) fail(`argument ${o.flag}: invalid float value: ${pyRepr(value)}`);
    return n;
  }
  return value;
}

// Python's repr() for the JSON-shaped values these scripts put into messages.
export function pyRepr(v) {
  if (v === null || v === undefined) return "None";
  if (v === true) return "True";
  if (v === false) return "False";
  if (typeof v === "number") return pyFloatOrInt(v);
  if (typeof v === "string") {
    const q = v.includes("'") && !v.includes('"') ? '"' : "'";
    let s = "";
    for (const ch of v) {
      const c = ch.codePointAt(0);
      if (ch === "\\") s += "\\\\";
      else if (ch === q) s += "\\" + q;
      else if (ch === "\n") s += "\\n";
      else if (ch === "\r") s += "\\r";
      else if (ch === "\t") s += "\\t";
      else if (c < 0x20 || c === 0x7f) s += "\\x" + c.toString(16).padStart(2, "0");
      else s += ch;
    }
    return q + s + q;
  }
  if (Array.isArray(v)) return "[" + v.map(pyRepr).join(", ") + "]";
  return "{" + Object.entries(v).map(([k, x]) => `${pyRepr(k)}: ${pyRepr(x)}`).join(", ") + "}";
}

function pyFloatOrInt(n) {
  return String(n);
}

// str() of a Python float: 120.0 stays "120.0", 2.5 stays "2.5".
export function pyFloat(n) {
  return Number.isInteger(n) && Math.abs(n) < 1e16 ? `${n}.0` : String(n);
}

// json.dumps(obj, indent=2): same layout as JSON.stringify(obj, null, 2), with non-ASCII escaped.
export function pyJson(obj) {
  return JSON.stringify(obj, null, 2).replace(/[^\x00-\x7f]/g, (ch) => "\\u" + ch.charCodeAt(0).toString(16).padStart(4, "0"));
}

// sys.exit(message): message to stderr, exit status 1.
export function die(msg) {
  process.stderr.write(`${msg}\n`);
  process.exit(1);
}
