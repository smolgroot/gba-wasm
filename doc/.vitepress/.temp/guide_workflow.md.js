import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"Development Workflow","description":"","frontmatter":{},"headers":[],"relativePath":"guide/workflow.md","filePath":"guide/workflow.md","lastUpdated":1792960225000}');
const _sfc_main = { name: "guide/workflow.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="development-workflow" tabindex="-1">Development Workflow <a class="header-anchor" href="#development-workflow" aria-label="Permalink to &quot;Development Workflow&quot;">​</a></h1><h2 id="standard-loop" tabindex="-1">Standard loop <a class="header-anchor" href="#standard-loop" aria-label="Permalink to &quot;Standard loop&quot;">​</a></h2><ol><li>Add or update a focused test in the Rust crate</li><li>Implement the corresponding subsystem in the emulator core</li><li>Run <code>cargo test</code> for the affected module</li><li>Build the wasm output for the browser bridge</li><li>Validate the app in the Vite dev environment</li></ol><h2 id="browser-shell" tabindex="-1">Browser shell <a class="header-anchor" href="#browser-shell" aria-label="Permalink to &quot;Browser shell&quot;">​</a></h2><p>The React app is responsible for UI, canvas rendering, and browser integration. It should not contain emulator logic beyond the minimal bridge and event handling required to drive the core.</p><h2 id="documentation" tabindex="-1">Documentation <a class="header-anchor" href="#documentation" aria-label="Permalink to &quot;Documentation&quot;">​</a></h2><p>The docs site lives under the <code>doc/</code> folder and is built with VitePress. It can be deployed to GitHub Pages with a static site configuration that preserves versioned release docs or a single docs root.</p></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("guide/workflow.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const workflow = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  workflow as default
};
