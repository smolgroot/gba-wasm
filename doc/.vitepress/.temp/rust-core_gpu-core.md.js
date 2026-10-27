import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"GPU Core","description":"","frontmatter":{},"headers":[],"relativePath":"rust-core/gpu-core.md","filePath":"rust-core/gpu-core.md","lastUpdated":1793054033000}');
const _sfc_main = { name: "rust-core/gpu-core.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="gpu-core" tabindex="-1">GPU Core <a class="header-anchor" href="#gpu-core" aria-label="Permalink to &quot;GPU Core&quot;">​</a></h1><p>The GPU core selects the modeled display mode from <code>DISPCNT</code> and currently renders GBA Mode 3 pixels from VRAM into an RGBA framebuffer.</p><h2 id="data-structure" tabindex="-1">Data structure <a class="header-anchor" href="#data-structure" aria-label="Permalink to &quot;Data structure&quot;">​</a></h2><p>The <code>GpuCore</code> stores a <code>framebuffer</code> as a flat RGBA array of length <code>SCREEN_WIDTH * SCREEN_HEIGHT * 4</code>.</p><h2 id="operations" tabindex="-1">Operations <a class="header-anchor" href="#operations" aria-label="Permalink to &quot;Operations&quot;">​</a></h2><ul><li><code>clear()</code> resets the framebuffer</li><li><code>set_pixel(x, y, rgba)</code> writes an RGBA pixel into the framebuffer</li><li><code>render(memory)</code> reads <code>DISPCNT</code>; Mode 3 renders RGB555 VRAM, while unsupported modes currently produce an opaque black frame</li><li><code>render_mode3(memory)</code> reads 240 x 160 little-endian RGB555 pixels from VRAM</li></ul><p>Each 5-bit color channel is expanded to 8 bits, and output pixels receive a fully opaque alpha channel.</p><h2 id="browser-integration" tabindex="-1">Browser integration <a class="header-anchor" href="#browser-integration" aria-label="Permalink to &quot;Browser integration&quot;">​</a></h2><p>The Rust core owns VRAM and display-control state. The browser receives only the resulting framebuffer bytes for canvas rendering.</p><h2 id="test-coverage" tabindex="-1">Test coverage <a class="header-anchor" href="#test-coverage" aria-label="Permalink to &quot;Test coverage&quot;">​</a></h2><p>Unit tests cover exact RGB555 decoding, <code>DISPCNT</code> mode selection, and the opaque-black fallback for unsupported modes.</p></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("rust-core/gpu-core.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const gpuCore = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  gpuCore as default
};
