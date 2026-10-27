import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"Introduction","description":"","frontmatter":{},"headers":[],"relativePath":"guide/introduction.md","filePath":"guide/introduction.md","lastUpdated":1792960225000}');
const _sfc_main = { name: "guide/introduction.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="introduction" tabindex="-1">Introduction <a class="header-anchor" href="#introduction" aria-label="Permalink to &quot;Introduction&quot;">​</a></h1><p>GBA WASM is a browser-first Game Boy Advance emulation project built with Rust and WebAssembly. The project is organized around a clean boundary: the emulator logic lives in Rust, while the app shell is delivered through a modern web stack.</p><h2 id="goals" tabindex="-1">Goals <a class="header-anchor" href="#goals" aria-label="Permalink to &quot;Goals&quot;">​</a></h2><p>The project aims to:</p><ul><li>run a GBA emulator in a browser using WebAssembly</li><li>keep the emulator core deterministic and testable</li><li>separate browser glue from the emulator logic</li><li>support responsive UI and installable PWA behavior</li><li>document the architecture in a static docs site hosted through GitHub Pages</li></ul><h2 id="current-status" tabindex="-1">Current status <a class="header-anchor" href="#current-status" aria-label="Permalink to &quot;Current status&quot;">​</a></h2><p>The codebase is in its first milestone: a Rust memory map and GPU framebuffer core. This foundation supports future CPU, cartridge, and timing work without forcing browser-specific concerns into the emulator core.</p><h2 id="recommended-workflow" tabindex="-1">Recommended workflow <a class="header-anchor" href="#recommended-workflow" aria-label="Permalink to &quot;Recommended workflow&quot;">​</a></h2><ol><li>Implement one emulator subsystem at a time in Rust</li><li>Add failing tests before broad feature logic</li><li>Expose only the minimal wasm boundary needed by the app</li><li>validate the browser shell separately from emulator correctness</li></ol></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("guide/introduction.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const introduction = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  introduction as default
};
