import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"Project Goals","description":"","frontmatter":{},"headers":[],"relativePath":"guide/goals.md","filePath":"guide/goals.md","lastUpdated":1792960225000}');
const _sfc_main = { name: "guide/goals.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="project-goals" tabindex="-1">Project Goals <a class="header-anchor" href="#project-goals" aria-label="Permalink to &quot;Project Goals&quot;">​</a></h1><h2 id="core-direction" tabindex="-1">Core direction <a class="header-anchor" href="#core-direction" aria-label="Permalink to &quot;Core direction&quot;">​</a></h2><p>The project is designed to make a GBA emulator accessible in the browser while keeping the emulator logic robust and future-proof.</p><h2 id="functional-goals" tabindex="-1">Functional goals <a class="header-anchor" href="#functional-goals" aria-label="Permalink to &quot;Functional goals&quot;">​</a></h2><ul><li>render a GBA framebuffer in the browser</li><li>expose Rust-based state to the UI layer through a thin wasm bridge</li><li>support responsive controls and a PWA-friendly app shell</li><li>preserve emulator behavior through explicit memory and timing models</li></ul><h2 id="engineering-goals" tabindex="-1">Engineering goals <a class="header-anchor" href="#engineering-goals" aria-label="Permalink to &quot;Engineering goals&quot;">​</a></h2><ul><li>keep the Rust crate independent from React or browser code</li><li>use tiny, focused tests for subsystem correctness</li><li>maintain a stable WebAssembly API surface for the frontend</li><li>document architecture and architecture decisions in public docs</li></ul></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("guide/goals.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const goals = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  goals as default
};
