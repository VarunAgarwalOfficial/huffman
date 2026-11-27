// Demo page: all Huffman logic runs in huffman.wasm (the Rust library); this file only draws.
// The tree is drawn as a small pixel tree: the root is the trunk on the ground, joins are knots
// on the branches and characters are leaves at the top. Labels are page text over the canvas.

import {
  dither,
  PALETTE as P,
  setupCanvas,
} from "https://varunswaika.github.io/varun-ui/shell/pixel.js";

const $ = (id) => document.getElementById(id);
// One art pixel is this many CSS pixels, the shell's --px-u.
const UNIT = 3;
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
  return `the knot holding ${model.merges[id - leaves].weight}`;
}

/** Fixed positions for every node of the finished tree: root on the ground, depth going up. */
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

/** A line of square stamps from (x0, y0) to (x1, y1), in art pixels. */
function line(g, x0, y0, x1, y1, size, colour) {
  g.fillStyle = colour;
  const dx = Math.abs(x1 - x0);
  const dy = -Math.abs(y1 - y0);
  const sx = x0 < x1 ? 1 : -1;
  const sy = y0 < y1 ? 1 : -1;
  const half = Math.floor(size / 2);
  let err = dx + dy;
  for (;;) {
    g.fillRect(x0 - half, y0 - half, size, size);
    if (x0 === x1 && y0 === y1) return;
    const e2 = 2 * err;
    if (e2 >= dy) {
      err += dy;
      x0 += sx;
    }
    if (e2 <= dx) {
      err += dx;
      y0 += sy;
    }
  }
}

/** A box with its corners cut, filled and outlined, centred on (x, y). */
function blob(g, x, y, w, h, fill, edge, round = 1) {
  // Odd sizes, so the shape is centred on the pixel (x, y).
  w |= 1;
  h |= 1;
  const left = x - (w >> 1);
  const top = y - (h >> 1);
  g.fillStyle = edge;
  g.fillRect(left + round, top, w - 2 * round, h);
  g.fillRect(left, top + round, w, h - 2 * round);
  if (round > 1) {
    g.fillRect(left + 1, top + 1, w - 2, h - 2);
  }
  g.fillStyle = fill;
  g.fillRect(left + round, top + 1, w - 2 * round, h - 2);
  g.fillRect(left + 1, top + round, w - 2, h - 2 * round);
  return { left, top, w, h };
}

/** A leaf: a green clump with a lighter top and a few darker pixels low down. */
function leaf(g, x, y, w, h, state) {
  const fill = state === "pair" ? P.measured : state === "old" ? P.meadow : P.meadowSoft;
  const edge = state === "pair" ? P.sunflowerDeep : state === "old" ? P.leaf : P.meadowMid;
  const box = blob(g, x, y, w, h, fill, edge, 2);
  // A short stem under the leaf, and a light streak along its top edge.
  g.fillStyle = edge;
  g.fillRect(x, box.top + box.h, 1, 1);
  if (state === "pair") return;
  g.fillStyle = state === "old" ? P.meadowSoft : P.plate;
  g.fillRect(box.left + 3, box.top + 1, Math.max(1, box.w - 8), 1);
}

/** A join: a knot of wood on the branch. */
function knot(g, x, y, w, h, state) {
  const fills = { pair: P.measured, made: P.open, old: P.wood2, "": P.wood1 };
  const edges = { pair: P.sunflowerDeep, made: P.inkDeep, old: P.wood6, "": P.wood4 };
  const box = blob(g, x, y, w, h, fills[state], edges[state]);
  if (state === "old" || state === "") {
    g.fillStyle = state === "old" ? P.wood3 : P.wood2;
    g.fillRect(box.left + 2, box.top + box.h - 2, box.w - 4, 1);
  }
}

/** The ground: flat grass with a light dither, two tufts and a stone, all placed by hand. */
function ground(g, width, top, height) {
  g.fillStyle = P.meadow;
  g.fillRect(0, top, width, height);
  g.fillStyle = P.meadowMid;
  g.fillRect(0, top, width, 1);
  dither(g, 0, top + 2, width, height - 2, P.meadowMid, 0.1);
  const tuft = (x) => {
    g.fillStyle = P.leaf;
    g.fillRect(x, top - 2, 1, 2);
    g.fillRect(x + 2, top - 3, 1, 3);
    g.fillRect(x + 4, top - 1, 1, 1);
  };
  tuft(Math.round(width * 0.13));
  tuft(Math.round(width * 0.68));
  const stone = Math.round(width * 0.87);
  g.fillStyle = P.stone3;
  g.fillRect(stone, top - 2, 4, 2);
  g.fillStyle = P.stone2;
  g.fillRect(stone + 1, top - 3, 2, 1);
}

/** A label in the page's text face, over the canvas at art position (x, y). */
function tag(stage, text, x, y, kind) {
  const span = document.createElement("span");
  span.className = `tag ${kind}`;
  span.textContent = text;
  // The middle of art pixel (x, y).
  span.style.left = `${(x + 0.5) * UNIT}px`;
  span.style.top = `${(y + 0.5) * UNIT}px`;
  stage.append(span);
}

function drawTree() {
  const stage = $("tree");
  for (const child of [...stage.querySelectorAll(".tag")]) child.remove();
  const canvas = stage.querySelector("canvas");
  const leaves = model.symbols.length;
  const { x, depth, deepest } = layout();
  // Columns are sized in CSS pixels so every label fits at full size; a column is never
  // narrower than its label ("space" needs more room than "e"). A tree too wide for the panel
  // scrolls sideways rather than shrinking its labels.
  const room = stage.parentElement.clientWidth - 2;
  const names = model.symbols.map((symbol) => label(symbol.byte));
  const need = names.map((name) => Math.ceil(textWidth(name)) + 12);
  let cell = Math.max(27, Math.min(48, Math.floor(room / Math.max(leaves, 1))));
  // Every leaf keeps at least 6 px from its neighbours.
  const slots = (c) => need.map((n) => Math.max(c, n + 6));
  let slot = slots(cell);
  const sum = (list) => list.reduce((a, b) => a + b, 0);
  while (sum(slot) > room && cell > 24) slot = slots(--cell);
  // From here on everything is in art pixels.
  const art = (css) => Math.round(css / UNIT);
  const row = 16;
  const groundHeight = 9;
  const width = Math.max(art(sum(slot)), 40);
  const tall = 36 + deepest * row;
  const groundTop = tall - groundHeight;
  const rootY = groundTop - 13;
  const g = setupCanvas(canvas, width, tall, UNIT);
  stage.style.width = `${width * UNIT}px`;
  g.clearRect(0, 0, width, tall);

  const centre = new Array(leaves + model.merges.length).fill(0);
  let left = 0;
  for (const id of [...Array(leaves).keys()].sort((a, b) => x[a] - x[b])) {
    centre[id] = art(left + slot[id] / 2);
    left += slot[id];
  }
  model.merges.forEach(({ a, b }, k) => {
    centre[leaves + k] = Math.round((centre[a] + centre[b]) / 2);
  });
  const px = (id) => centre[id];
  const py = (id) => rootY - depth[id] * row;
  const current = step > 0 ? model.merges[step - 1] : null;
  const made = leaves + step - 1;
  const total = leaves + model.merges.length;
  const weightOf = (id) =>
    id < leaves ? model.symbols[id].count : model.merges[id - leaves].weight;
  const heaviest = Math.max(1, ...Array.from({ length: total }, (_, id) => weightOf(id)));
  const thickness = (id) => 1 + Math.round((2 * weightOf(id)) / heaviest);
  const done = leaves > 0 && step === model.merges.length;

  ground(g, width, groundTop, groundHeight);
  // The trunk, once the tree is one piece: from the root down into the ground, with roots.
  if (done) {
    const root = total - 1;
    const tx = px(root);
    for (let y = py(root); y < groundTop + 1; y++) {
      g.fillStyle = P.wood4;
      g.fillRect(tx - 2, y, 4, 1);
      g.fillStyle = P.wood5;
      g.fillRect(tx + 1, y, 1, 1);
    }
    g.fillStyle = P.wood5;
    g.fillRect(tx - 4, groundTop, 2, 1);
    g.fillRect(tx + 2, groundTop, 3, 1);
    g.fillRect(tx - 5, groundTop + 1, 1, 1);
    g.fillRect(tx + 5, groundTop + 1, 1, 1);
  }
  // Branches, each as thick as the share of the text it carries, with a darker side.
  for (let k = 0; k < step; k++) {
    const { a, b } = model.merges[k];
    for (const child of [a, b]) {
      const size = thickness(child);
      const x0 = px(leaves + k);
      const y0 = py(leaves + k);
      line(g, x0 + 1, y0, px(child) + 1, py(child) + 5, size, P.wood5);
      line(g, x0, y0, px(child), py(child) + 5, size, P.wood4);
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
    const weight = weightOf(id);
    if (isLeaf) {
      // Long names ("space") get a wider leaf, in a column wide enough to hold it.
      const wide = Math.min(art(slot[id]) - 3, Math.max(art(cell) - 3, art(need[id])));
      leaf(g, px(id), py(id), wide, 11, state);
      tag(stage, names[id], px(id), py(id), onColour);
      tag(stage, weight, px(id), py(id) - 9, "count");
    } else {
      const wide = Math.max(10, art(textWidth(String(weight)) + 14));
      knot(g, px(id), py(id), wide, 9, state);
      tag(stage, weight, px(id), py(id), onColour);
      if (state === "made") {
        // Below the knot, where its own parent will go; for the root, beside the trunk.
        const side = depth[id] === 0 ? Math.ceil(art(textWidth("new")) / 2) + 4 : 0;
        tag(stage, "new", px(id) + side, py(id) + 9, "count");
      }
    }
  }
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
    text = `Nothing has run yet. Each of the ${model.symbols.length} different characters in the text is a leaf with its count above it, placed at the height it will have in the finished tree. Press Play or Step forward to join the two rarest.`;
  else {
    const { a, b, weight } = model.merges[step - 1];
    text = `Join ${step} of ${total}: the two rarest left are ${nodeName(a)} and ${nodeName(b)}, so they are joined on a new knot holding their total, ${weight}.`;
    if (step === total)
      text +=
        " One tree is left, so it is finished. Each character's code length is how many levels up from the trunk it sits.";
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
