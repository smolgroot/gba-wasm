import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"GBA WASM","description":"","frontmatter":{},"headers":[],"relativePath":"index.md","filePath":"index.md","lastUpdated":1792960225000}');
const _sfc_main = { name: "index.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="gba-wasm" tabindex="-1">GBA WASM <a class="header-anchor" href="#gba-wasm" aria-label="Permalink to &quot;GBA WASM&quot;">​</a></h1><p>A WebAssembly-based Game Boy Advance emulator and browser playground built for responsive, modern web delivery.</p><h2 id="why-this-project-exists" tabindex="-1">Why this project exists <a class="header-anchor" href="#why-this-project-exists" aria-label="Permalink to &quot;Why this project exists&quot;">​</a></h2><p>This project brings together a browser UI and a high-performance Rust emulator core. The goal is to make GBA emulation available in a web environment without sacrificing the correctness, determinism, and performance benefits of native-style emulation logic.</p><h2 id="project-principles" tabindex="-1">Project principles <a class="header-anchor" href="#project-principles" aria-label="Permalink to &quot;Project principles&quot;">​</a></h2><ul><li>Rust for the emulator core and memory model</li><li>WebAssembly for browser execution</li><li>React + Vite for a responsive app shell</li><li>GitHub Pages for project documentation</li><li>Test-driven milestone development for CPU, GPU, and memory features</li></ul><h2 id="current-milestone" tabindex="-1">Current milestone <a class="header-anchor" href="#current-milestone" aria-label="Permalink to &quot;Current milestone&quot;">​</a></h2><p>The first milestone focuses on the core GBA memory map and GPU framebuffer layer:</p><ul><li>SRAM and working memory layout</li><li>GBA-style memory address mapping</li><li>GPU framebuffer representation</li><li>pixel drawing and test pattern generation</li><li>Rust unit coverage for the core model</li></ul><h2 id="documentation-layout" tabindex="-1">Documentation layout <a class="header-anchor" href="#documentation-layout" aria-label="Permalink to &quot;Documentation layout&quot;">​</a></h2><p>This site is organized into three sections:</p><ol><li>Getting Started: how the project is bootstrapped and developed</li><li>Architecture: how the emulator core, browser glue, and WebAssembly boundary fit together</li><li>Rust Core: memory, GPU, and testing details for the first implementation milestone</li></ol><h2 id="quick-links" tabindex="-1">Quick links <a class="header-anchor" href="#quick-links" aria-label="Permalink to &quot;Quick links&quot;">​</a></h2><ul><li>Setup and workflow</li><li>Memory model</li><li>GPU pipeline</li><li>Build and test commands</li></ul></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("index.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const index = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  index as default
};
