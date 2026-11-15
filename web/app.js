// Demo page: all Huffman logic runs in huffman.wasm (the Rust library); this file only draws.
// The host page follows the system colour scheme, so there is no theme button here.

const $ = (id) => document.getElementById(id);
const SVG = "http://www.w3.org/2000/svg";
const MAX_SHOWN = 400;
// Labels in the tree are set in the page's text face at the site's small size, never smaller.
const SMALL = 17;
const measure = document.createElement("canvas").getContext("2d");
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
  return `the circle holding ${model.merges[id - leaves].weight}`;
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

/** Width of a label at the small size, in the page's text face. */
function textWidth(text) {
  measure.font = `${SMALL}px ${getComputedStyle(document.body).fontFamily}`;
  return measure.measureText(text).width;
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
  // Narrow screens get narrower columns drawn at full size, so the labels stay readable.
  // A column is never narrower than its label ("space" needs more room than "e"); only a
  // tree with very many leaves is scaled down to fit.
  const room = svg.parentElement.clientWidth - 2;
  const names = model.symbols.map((symbol) => label(symbol.byte));
  const need = names.map((name) => Math.ceil(textWidth(name)) + 16);
  let cell = Math.max(28, Math.min(46, Math.floor(room / Math.max(leaves, 1))));
  // Every box keeps at least 8 px from its neighbours.
  const slots = (c) => need.map((n) => Math.max(c, n + 8));
  let slot = slots(cell);
  const sum = (list) => list.reduce((a, b) => a + b, 0);
  while (sum(slot) > room && cell > 24) slot = slots(--cell);
  const box = cell - 8;
  const rowHeight = Math.max(46, Math.round(cell * 1.26));
  const width = Math.max(sum(slot), cell);
  const tall = (deepest + 1) * rowHeight + 26;
  svg.setAttribute("viewBox", `0 0 ${width} ${tall}`);
  // Shrinks to fit a narrow screen instead of being cut off; never grows past its own size.
  svg.style.maxWidth = `${width}px`;
  // Leaves sit left to right in tree order, each in its own column; a join sits midway over its two.
  const centre = new Array(leaves + model.merges.length).fill(0);
  let left = 0;
  for (const id of [...Array(leaves).keys()].sort((a, b) => x[a] - x[b])) {
    centre[id] = left + slot[id] / 2;
    left += slot[id];
  }
  model.merges.forEach(({ a, b }, k) => {
    centre[leaves + k] = (centre[a] + centre[b]) / 2;
  });
  const px = (id) => centre[id];
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
      const name = names[id];
      // Long names ("space") get a wider box, in a column wide enough to hold it.
      const wide = Math.min(slot[id] - 8, Math.max(box, need[id]));
      svg.append(
        element("rect", {
          class: `node ${state}`,
          x: px(id) - wide / 2,
          y: py(id) - 14,
          width: wide,
          height: 28,
          rx: 2,
        }),
      );
      svg.append(
        element(
          "text",
          {
            class: onColour,
            x: px(id),
            y: py(id),
          },
          name,
        ),
      );
      svg.append(element("text", { class: "count", x: px(id), y: py(id) + 26 }, weight));
    } else {
      svg.append(
        element("circle", {
          class: `node ${state}`,
          cx: px(id),
          cy: py(id),
          r: Math.max(13, Math.min(16, box / 2 + 2)),
        }),
      );
      svg.append(element("text", { class: onColour, x: px(id), y: py(id) }, weight));
      if (state === "made") {
        // Above the circle, or beside it for the top of the tree, where there is no room above.
        const top = depth[id] === 0;
        svg.append(
          element(
            "text",
            {
              class: "count",
              x: top ? px(id) + 36 : px(id),
              y: top ? py(id) : py(id) - 26,
            },
            "new",
          ),
        );
      }
    }
  }
  const done = step === model.merges.length;
  $("tree-title").textContent = done
    ? `Finished Huffman tree with ${leaves} leaves`
    : `Huffman tree after ${step} of ${model.merges.length} merges`;
}

function drawStatus() {
  const total = model.merges.length;
  $("position-label").textContent = `Joins done: ${step} of ${total}`;
  $("position").max = total;
  $("position").value = step;
  let text;
  if (model.symbols.length === 0) text = "Nothing to encode: no symbols, no tree, no bits.";
  else if (model.symbols.length === 1)
    text =
      "Only one distinct symbol, so there is nothing to merge. It still gets a one-bit code, so that every symbol costs a bit.";
  else if (step === 0)
    text = `Nothing has run yet. Each of the ${model.symbols.length} different characters in the text is a box with its count under it, placed at the height it will have in the finished tree. Press Play or Step forward to join the two rarest.`;
  else {
    const { a, b, weight } = model.merges[step - 1];
    text = `Join ${step} of ${total}: the two rarest left are ${nodeName(a)} and ${nodeName(b)}, so they are joined under a new circle holding their total, ${weight}.`;
    if (step === total)
      text +=
        " One tree is left, so it is finished. Each character's code length is how many levels down it sits.";
  }
  $("status").textContent = text;
  for (const [id, off] of [
    ["back", step === 0],
    ["step", step === total],
    ["play", total === 0],
  ]) {
    $(id).setAttribute("aria-disabled", String(off));
  }
  const done = total > 0 && step === total;
  for (const node of document.querySelectorAll(".when-done")) node.hidden = !done && total > 0;
  for (const node of document.querySelectorAll(".until-done")) node.hidden = done || total === 0;
  drawTotals(done || total === 0);
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

  fill("more-totals", [
    ["Lower limit (entropy)", `${(model.entropy * bytes).toFixed(1)} bits`],
    ["As a file", `${model.containerBytes} bytes (${model.overheadBytes} are table and checksums)`],
  ]);
}

/** Fills a list of figures: each is a term and its value. */
function fill(id, figures) {
  $(id).replaceChildren(
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

/** The headline sizes. The coded size is only known once the tree is finished. */
function drawTotals(done) {
  const { bytes } = model;
  const waiting = "when the tree is finished";
  fill("totals", [
    ["Plain, 8 bits per character", `${bytes * 8} bits`],
    ["Huffman coded", done ? `${model.payloadBits} bits` : waiting],
    [
      "Average code length",
      done ? (bytes ? `${model.average.toFixed(2)} bits per character` : "none") : waiting,
    ],
  ]);
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
  // Start before the first join: nothing is shown finished until it has been built.
  go(0);
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
  // The labels are measured in the page's face, so draw again once it has loaded.
  document.fonts.ready.then(drawTree);
  // Redraws only when the panel changes width, never while idle.
  let width = $("tree").parentElement.clientWidth;
  new ResizeObserver(() => {
    const now = $("tree").parentElement.clientWidth;
    if (now !== width) {
      width = now;
      drawTree();
    }
  }).observe($("tree").parentElement);
}

main().catch((error) => {
  $("status").textContent = `Could not start the demo: ${error.message}`;
});
