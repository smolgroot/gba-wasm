import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"GPU Pipeline","description":"","frontmatter":{},"headers":[],"relativePath":"architecture/gpu-pipeline.md","filePath":"architecture/gpu-pipeline.md","lastUpdated":1792960877000}');
const _sfc_main = { name: "architecture/gpu-pipeline.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="gpu-pipeline" tabindex="-1">GPU Pipeline <a class="header-anchor" href="#gpu-pipeline" aria-label="Permalink to &quot;GPU Pipeline&quot;">​</a></h1><p>The current GPU milestone implements GBA Mode 3: a 240 x 160 bitmap with 16-bit RGB555 pixels stored in VRAM. This provides a deterministic first rendering path before adding tile, background, and object modes.</p><h2 id="current-representation" tabindex="-1">Current representation <a class="header-anchor" href="#current-representation" aria-label="Permalink to &quot;Current representation&quot;">​</a></h2><p>The <code>GpuCore</code> stores a <code>framebuffer</code> of RGBA pixel data with dimensions matching the GBA screen: 240 x 160.</p><h2 id="capabilities" tabindex="-1">Capabilities <a class="header-anchor" href="#capabilities" aria-label="Permalink to &quot;Capabilities&quot;">​</a></h2><ul><li>clear the framebuffer</li><li>set a single pixel</li><li>decode RGB555 pixels from VRAM</li><li>expose RGBA framebuffer bytes to the browser through WebAssembly</li></ul><h2 id="design-intent" tabindex="-1">Design intent <a class="header-anchor" href="#design-intent" aria-label="Permalink to &quot;Design intent&quot;">​</a></h2><p>The GPU model intentionally stays close to the raw framebuffer instead of embedding browser logic. This makes the Rust implementation testable and reusable when the full GBA pipeline is implemented.</p></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("architecture/gpu-pipeline.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const gpuPipeline = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  gpuPipeline as default
};
