// The wasm module of shem, loaded as the browser loads it (docs/adr/0018).
//
//   SHEM_WASM=<shem.wasm> SHEM_PPM=<directory> deno test --allow-read --allow-env wasm/test.ts
//
// SHEM_PPM holds the pages of kanikosen that the native shem wrote with
// `shem render --dpi 144 --format ppm`.

const root = new URL("..", import.meta.url).pathname;
const wasmPath = Deno.env.get("SHEM_WASM") ??
  `${root}target/wasm32-unknown-unknown/release/shem.wasm`;
const ppmDir = Deno.env.get("SHEM_PPM") ?? "";
const kanikosen = `${root}tests/fixtures/golem/kanikosen.pdf`;

type Exports = {
  memory: WebAssembly.Memory;
  shem_alloc(len: number): number;
  shem_free(ptr: number): void;
  shem_open(ptr: number, len: number, dpi: number): number;
  shem_page_count(doc: number): number;
  shem_render(doc: number, page: number): number;
  shem_image_ptr(doc: number): number;
  shem_image_width(doc: number): number;
  shem_image_height(doc: number): number;
  shem_close(doc: number): void;
  shem_error_ptr(): number;
  shem_error_len(): number;
};

const names = [
  "memory",
  "shem_alloc",
  "shem_close",
  "shem_error_len",
  "shem_error_ptr",
  "shem_free",
  "shem_image_height",
  "shem_image_ptr",
  "shem_image_width",
  "shem_open",
  "shem_page_count",
  "shem_render",
];

const module = await WebAssembly.compile(await Deno.readFile(wasmPath));

const instance = async (): Promise<Exports> =>
  (await WebAssembly.instantiate(module, {})).exports as unknown as Exports;

const error = (ex: Exports): string =>
  new TextDecoder().decode(
    new Uint8Array(ex.memory.buffer, ex.shem_error_ptr(), ex.shem_error_len()),
  );

const open = (ex: Exports, pdf: Uint8Array, dpi: number): number => {
  const ptr = ex.shem_alloc(pdf.length);
  new Uint8Array(ex.memory.buffer, ptr, pdf.length).set(pdf);
  const doc = ex.shem_open(ptr, pdf.length, dpi);
  ex.shem_free(ptr);
  return doc;
};

const image = (ex: Exports, doc: number) => {
  const width = ex.shem_image_width(doc);
  const height = ex.shem_image_height(doc);
  const rgba = new Uint8Array(
    ex.memory.buffer,
    ex.shem_image_ptr(doc),
    width * height * 4,
  ).slice();
  return { width, height, rgba };
};

const assert = (ok: boolean, message: string) => {
  if (!ok) throw new Error(message);
};

/// The pixels of a binary PPM, after its three header lines.
const ppmPixels = (ppm: Uint8Array): Uint8Array => {
  let lines = 0;
  let i = 0;
  while (lines < 3) {
    if (ppm[i] === 0x0a) lines++;
    i++;
  }
  return ppm.subarray(i);
};

Deno.test("the module has no imports and exports the functions", () => {
  assert(
    WebAssembly.Module.imports(module).length === 0,
    JSON.stringify(WebAssembly.Module.imports(module)),
  );
  // The linker of rustc also exports two globals, which are not used.
  const exports = WebAssembly.Module.exports(module);
  const used = exports.filter((e) => e.kind !== "global").map((e) => e.name)
    .sort();
  assert(JSON.stringify(used) === JSON.stringify(names), used.join(" "));
  const globals = exports.filter((e) => e.kind === "global").map((e) => e.name)
    .sort();
  assert(
    JSON.stringify(globals) === JSON.stringify(["__data_end", "__heap_base"]),
    globals.join(" "),
  );
});

Deno.test("kanikosen at 144 dpi is the native rendering, page by page", async () => {
  const ex = await instance();
  const doc = open(ex, await Deno.readFile(kanikosen), 144);
  assert(doc >= 1, error(ex));
  const count = ex.shem_page_count(doc);
  assert(count === 129, `${count} pages`);
  for (let page = 0; page < count; page++) {
    assert(ex.shem_render(doc, page) === 1, error(ex));
    const { width, height, rgba } = image(ex, doc);
    // B6 of JIS, 128 × 182 mm, at 144 dpi: 725.67 × 1031.81, rounded up.
    assert(width === 726 && height === 1032, `${width} × ${height}`);
    if (ppmDir !== "") {
      const name = `${ppmDir}/page-${String(page + 1).padStart(4, "0")}.ppm`;
      const rgb = ppmPixels(await Deno.readFile(name));
      assert(rgb.length * 4 === rgba.length * 3, `page ${page + 1}: length`);
      for (let i = 0, j = 0; i < rgba.length; i += 4, j += 3) {
        if (
          rgba[i] !== rgb[j] || rgba[i + 1] !== rgb[j + 1] ||
          rgba[i + 2] !== rgb[j + 2] || rgba[i + 3] !== 255
        ) {
          throw new Error(`page ${page + 1}: pixel ${i / 4} differs`);
        }
      }
    }
  }
  ex.shem_close(doc);
});

Deno.test("pages in any order are the same images", async () => {
  const ex = await instance();
  const doc = open(ex, await Deno.readFile(kanikosen), 36);
  const order = [0, 1, 2, 3, 3, 2, 1, 0];
  const images = order.map((page) => {
    assert(ex.shem_render(doc, page) === 1, error(ex));
    return image(ex, doc).rgba;
  });
  order.forEach((page, k) => {
    const other = images[order.lastIndexOf(page)];
    assert(
      images[k].every((v, i) => v === other[i]),
      `page ${page + 1}`,
    );
  });
});

Deno.test("broken PDFs fail with a message, without a trap", async () => {
  const ex = await instance();
  const pdf = await Deno.readFile(kanikosen);
  const inputs = [
    pdf.subarray(0, 1000),
    pdf.subarray(0, pdf.length - 1000),
    new TextEncoder().encode("not a PDF"),
    new Uint8Array(0),
  ];
  for (const input of inputs) {
    const doc = open(ex, input, 72);
    if (doc === 0) {
      assert(error(ex).length > 0, "no message");
      continue;
    }
    for (let page = 0; page < Math.min(ex.shem_page_count(doc), 3); page++) {
      if (ex.shem_render(doc, page) === 0) {
        assert(error(ex).length > 0, "no message");
      }
    }
  }
  assert(ex.shem_render(9999, 0) === 0, "no such document");
  assert(error(ex) === "no such document: 9999", error(ex));
});
