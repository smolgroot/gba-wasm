import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"WASM Boundary","description":"","frontmatter":{},"headers":[],"relativePath":"architecture/wasm-boundary.md","filePath":"architecture/wasm-boundary.md","lastUpdated":1792962236000}');
const _sfc_main = { name: "architecture/wasm-boundary.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="wasm-boundary" tabindex="-1">WASM Boundary <a class="header-anchor" href="#wasm-boundary" aria-label="Permalink to &quot;WASM Boundary&quot;">​</a></h1><p>The WebAssembly boundary keeps the browser app and the emulator core separate.</p><h2 id="exported-functions" tabindex="-1">Exported functions <a class="header-anchor" href="#exported-functions" aria-label="Permalink to &quot;Exported functions&quot;">​</a></h2><p>The Rust crate exposes small functions such as:</p><ul><li><code>hello(name: &amp;str)</code> for smoke testing</li><li><code>run_thumb_rom(rom, steps)</code> to execute a Thumb test program and return an RGBA frame</li><li><code>init()</code> for runtime setup</li></ul><h2 id="data-flow" tabindex="-1">Data flow <a class="header-anchor" href="#data-flow" aria-label="Permalink to &quot;Data flow&quot;">​</a></h2><p>The browser loads the wasm module, calls exported functions, and receives raw RGBA byte arrays. The React layer then paints those bytes to a canvas without needing direct emulator internals in the UI.</p><h2 id="why-this-boundary-matters" tabindex="-1">Why this boundary matters <a class="header-anchor" href="#why-this-boundary-matters" aria-label="Permalink to &quot;Why this boundary matters&quot;">​</a></h2><p>A narrow API surface reduces accidental cross-layer coupling and makes the core easier to test in Rust without browser dependencies.</p></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("architecture/wasm-boundary.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const wasmBoundary = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  wasmBoundary as default
};
