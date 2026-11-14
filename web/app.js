// Demo page: all Huffman logic runs in huffman.wasm (the Rust library); this file only draws.
import { initTheme } from "./theme.js";

const $ = (id) => document.getElementById(id);
const SVG = "http://www.w3.org/2000/svg";
const MAX_SHOWN = 400;
const encoder = new TextEncoder();

let wasm;
let model = { symbols: [], merges: [], bytes: 0 };
let input = new Uint8Array();
let step = 0;
let timer = 0;

/** Calls the Rust library and returns its JSON description of the code. */
function analyze(bytes) {
  const { memory, huff_alloc, huff_free, huff_analyze } = wasm;
  const source = huff_alloc(bytes.length);
  new Uint8Array(memory.buffer, source, bytes.length).set(bytes);
  const result = huff_analyze(source, bytes.length);
  huff_free(source, bytes.length);
  const length = new DataView(memory.buffer).getUint32(result, true);
  const json = new TextDecoder().decode(new Uint8Array(memory.buffer, result + 4, length));
  huff_free(result, 4 + length);
  return JSON.parse(json);
}

/** A short readable name for a byte value. */
function label(byte) {
  if (byte === 32) return "space";
  if (byte === 10) return "\\n";
  if (byte === 9) return "\\t";
  if (byte > 32 && byte < 127) return String.fromCharCode(byte);
  return byte.toString(16).padStart(2, "0");
}

function nodeName(id) {
  const leaves = model.symbols.length;
  if (id < leaves) return `${label(model.symbols[id].byte)} (${model.symbols[id].count})`;
  return `node ${id - leaves + 1} (${model.merges[id - leaves].weight})`;
}

/** Fixed positions for every node of the finished tree: root at the top, depth going down. */
function layout() {
  const leaves = model.symbols.length;
  const total = leaves + model.merges.length;
  const x = new Array(total).fill(0);
  const depth = new Array(total).fill(0);
  let next = 0;
  let deepest = 0;
  const place = (id, level) => {
    depth[id] = level;
    deepest = Math.max(deepest, level);
    if (id < leaves) {
      x[id] = next++;
      return;
    }
    const { a, b } = model.merges[id - leaves];
    place(a, level + 1);
    place(b, level + 1);
    x[id] = (x[a] + x[b]) / 2;
  };
  if (total > 0) place(total - 1, 0);
  return { x, depth, deepest };
}

function element(name, attributes, text) {
  const node = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attributes)) node.setAttribute(key, value);
  if (text !== undefined) node.textContent = text;
  return node;
}

function drawTree() {
  const svg = $("tree");
  for (const child of [...svg.children]) if (child.tagName !== "title") child.remove();
  const leaves = model.symbols.length;
  const { x, depth, deepest } = layout();
  const cell = 46;
  const rowHeight = 58;
  const width = Math.max(leaves, 1) * cell;
  const tall = (deepest + 1) * rowHeight + 16;
  svg.setAttribute("viewBox", `0 0 ${width} ${tall}`);
  svg.setAttribute("width", width);
  svg.setAttribute("height", tall);
  const px = (id) => x[id] * cell + cell / 2;
  const py = (id) => depth[id] * rowHeight + 22;
  const current = step > 0 ? model.merges[step - 1] : null;
  const made = leaves + step - 1;

  for (let k = 0; k < step; k++) {
    const { a, b } = model.merges[k];
    for (const child of [a, b]) {
      svg.append(
        element("line", {
          class: "edge",
          x1: px(leaves + k),
          y1: py(leaves + k),
          x2: px(child),
          y2: py(child),
        }),
      );
    }
  }
  for (let id = 0; id < leaves + step; id++) {
    const isLeaf = id < leaves;
    const state =
      current && (id === current.a || id === current.b)
        ? "pair"
        : current && id === made
          ? "made"
          : id < made
            ? "old"
            : "";
    const onColour = state === "pair" || state === "made" ? "on-colour" : "";
    const weight = isLeaf ? model.symbols[id].count : model.merges[id - leaves].weight;
    if (isLeaf) {
      svg.append(
        element("rect", {
          class: `node ${state}`,
          x: px(id) - 19,
          y: py(id) - 14,
          width: 38,
          height: 28,
          rx: 5,
        }),
      );
      const name = label(model.symbols[id].byte);
      svg.append(
        element(
          "text",
          { class: onColour, x: px(id), y: py(id), "font-size": name.length > 2 ? 10 : 13 },
          name,
        ),
      );
      svg.append(element("text", { class: "count", x: px(id), y: py(id) + 25 }, weight));
    } else {
      svg.append(element("circle", { class: `node ${state}`, cx: px(id), cy: py(id), r: 15 }));
      svg.append(element("text", { class: onColour, x: px(id), y: py(id) }, weight));
    }
  }
  const done = step === model.merges.length;
  $("tree-title").textContent = done
    ? `Finished Huffman tree with ${leaves} leaves`
    : `Huffman tree after ${step} of ${model.merges.length} merges`;
}

function drawStatus() {
  const total = model.merges.length;
  $("position-label").textContent = `Merge ${step} of ${total}`;
  $("position").max = total;
  $("position").value = step;
  let text;
  if (model.symbols.length === 0) text = "Nothing to encode: no symbols, no tree, no bits.";
  else if (model.symbols.length === 1)
    text =
      "Only one distinct symbol, so there is nothing to merge. It still gets a one-bit code, so that every symbol costs a bit.";
  else if (step === 0)
    text = `Start: ${model.symbols.length} symbols, each a tree of its own, weighted by its count.`;
  else {
    const { a, b, weight } = model.merges[step - 1];
    text = `Merge ${step}: the two lightest are ${nodeName(a)} and ${nodeName(b)}. Join them under a new node of weight ${weight}.`;
    if (step === total) text += " One tree is left: done.";
  }
  $("status").textContent = text;
  for (const [id, off] of [
    ["back", step === 0],
    ["step", step === total],
    ["play", total === 0],
  ]) {
    $(id).setAttribute("aria-disabled", String(off));
  }
  [...$("merges").children].forEach((item, k) => {
    item.className = k < step ? "" : "pending";
    if (k === step - 1) item.setAttribute("aria-current", "step");
    else item.removeAttribute("aria-current");
  });
}

function drawStatic() {
  const { symbols, bytes } = model;
  const rows = [...symbols].sort(
    (p, q) => q.count - p.count || p.code.length - q.code.length || p.byte - q.byte,
  );
  $("table").replaceChildren(
    ...rows.map((s) => {
      const row = document.createElement("tr");
      for (const [text, kind] of [
        [label(s.byte), "symbol"],
        [s.count, ""],
        [s.code.length, ""],
        [s.code, "code"],
      ]) {
        const cell = document.createElement("td");
        cell.textContent = text;
        cell.className = kind;
        row.append(cell);
      }
      return row;
    }),
  );
  $("merges").replaceChildren(
    ...model.merges.map(({ a, b, weight }) => {
      const item = document.createElement("li");
      item.textContent = `${nodeName(a)} + ${nodeName(b)} = ${weight}`;
      return item;
    }),
  );

  const codes = new Map(symbols.map((s) => [s.byte, s.code]));
  const shown = input.subarray(0, MAX_SHOWN);
  $("bits").replaceChildren(
    ...Array.from(shown, (byte) => {
      const group = document.createElement("li");
      const name = document.createElement("small");
      name.textContent = label(byte);
      group.append(codes.get(byte) ?? "", name);
      return group;
    }),
  );
  $("bits-note").textContent =
    bytes === 0
      ? "No text, no bits."
      : bytes > MAX_SHOWN
        ? `Showing the first ${MAX_SHOWN} of ${bytes} symbols. Each group is one symbol: its code above, the symbol below.`
        : "Each group is one symbol: its code above, the symbol below. No code is the start of another, so no separators are needed.";

  const plain = bytes * 8;
  const figures = [
    ["Plain", `${plain} bits`],
    ["Huffman code words", `${model.payloadBits} bits`],
    ["Entropy bound", `${(model.entropy * bytes).toFixed(1)} bits`],
    ["Average code", bytes ? `${model.average.toFixed(3)} bits/symbol` : "n/a"],
    ["As a file", `${model.containerBytes} bytes (${model.overheadBytes} are table and checksums)`],
  ];
  $("totals").replaceChildren(
    ...figures.map(([term, value]) => {
      const box = document.createElement("div");
      const dt = document.createElement("dt");
      const dd = document.createElement("dd");
      dt.textContent = term;
      dd.textContent = value;
      box.append(dt, dd);
      return box;
    }),
  );
}

function stop() {
  clearInterval(timer);
  timer = 0;
  $("play").textContent = "Play";
}

function go(to) {
  step = Math.max(0, Math.min(model.merges.length, to));
  drawTree();
  drawStatus();
  if (step === model.merges.length) stop();
}

function refresh() {
  stop();
  input = encoder.encode($("text").value);
  model = analyze(input);
  drawStatic();
  // Show the finished tree first; Play rebuilds it from the start.
  go(model.merges.length);
}

function play() {
  if (timer) return stop();
  if (model.merges.length === 0) return;
  if (step === model.merges.length) go(0);
  $("play").textContent = "Pause";
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  timer = setInterval(() => go(step + 1), still ? 1600 : 900);
}

async function main() {
  initTheme($("theme"));
  const { instance } = await WebAssembly.instantiateStreaming(fetch("huffman.wasm"));
  wasm = instance.exports;
  $("text").addEventListener("input", refresh);
  const jump = (to) => {
    stop();
    go(to);
  };
  $("back").addEventListener("click", () => jump(step - 1));
  $("step").addEventListener("click", () => jump(step + 1));
  $("play").addEventListener("click", play);
  $("position").addEventListener("input", (event) => jump(Number(event.target.value)));
  refresh();
}

main().catch((error) => {
  $("status").textContent = `Could not start the demo: ${error.message}`;
});
