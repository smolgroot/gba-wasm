import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"Architecture Overview","description":"","frontmatter":{},"headers":[],"relativePath":"architecture/overview.md","filePath":"architecture/overview.md","lastUpdated":1792960225000}');
const _sfc_main = { name: "architecture/overview.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="architecture-overview" tabindex="-1">Architecture Overview <a class="header-anchor" href="#architecture-overview" aria-label="Permalink to &quot;Architecture Overview&quot;">​</a></h1><p>The project is split into a few clear concerns:</p><ul><li><code>rust/</code>: emulator core logic, memory model, GPU, and test suite</li><li><code>src/</code>: browser application code and wasm bridge</li><li><code>public/</code>: generated wasm assets and runtime app resources</li><li><code>doc/</code>: public project documentation built with VitePress</li></ul><h2 id="separation-of-responsibilities" tabindex="-1">Separation of responsibilities <a class="header-anchor" href="#separation-of-responsibilities" aria-label="Permalink to &quot;Separation of responsibilities&quot;">​</a></h2><h3 id="emulator-core" tabindex="-1">Emulator core <a class="header-anchor" href="#emulator-core" aria-label="Permalink to &quot;Emulator core&quot;">​</a></h3><p>The Rust crate owns simulation correctness: reading and writing memory, processing rendering state, and modeling game-system behavior.</p><h3 id="browser-layer" tabindex="-1">Browser layer <a class="header-anchor" href="#browser-layer" aria-label="Permalink to &quot;Browser layer&quot;">​</a></h3><p>The app shell owns user experience: controls, canvas presentation, input handling, and the browser-specific environment.</p><h3 id="wasm-boundary" tabindex="-1">WASM boundary <a class="header-anchor" href="#wasm-boundary" aria-label="Permalink to &quot;WASM boundary&quot;">​</a></h3><p>The boundary between the browser and the Rust core is intentionally narrow. The app loads the wasm module and calls small exported functions that return plain data structures like bytes or basic values.</p></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("architecture/overview.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const overview = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  overview as default
};
