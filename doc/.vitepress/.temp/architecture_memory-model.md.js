import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"Memory Model","description":"","frontmatter":{},"headers":[],"relativePath":"architecture/memory-model.md","filePath":"architecture/memory-model.md","lastUpdated":1792960225000}');
const _sfc_main = { name: "architecture/memory-model.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="memory-model" tabindex="-1">Memory Model <a class="header-anchor" href="#memory-model" aria-label="Permalink to &quot;Memory Model&quot;">​</a></h1><p>The Game Boy Advance memory map is a foundational subsystem. The current implementation models a representative memory layout that supports the emulator core and future CPU access.</p><h2 id="regions-in-scope" tabindex="-1">Regions in scope <a class="header-anchor" href="#regions-in-scope" aria-label="Permalink to &quot;Regions in scope&quot;">​</a></h2><ul><li>EWRAM: 256 KiB external work RAM</li><li>IWRAM: 32 KiB internal work RAM</li><li>VRAM: 96 KiB video RAM</li><li>OAM: 1 KiB object attribute memory</li><li>palette memory: 1 KiB color table</li><li>I/O region: mapped control registers and system state</li></ul><h2 id="read-and-write-behavior" tabindex="-1">Read and write behavior <a class="header-anchor" href="#read-and-write-behavior" aria-label="Permalink to &quot;Read and write behavior&quot;">​</a></h2><p>The core memory abstraction exposes <code>read_u8</code>, <code>write_u8</code>, <code>read_u16</code>, and <code>write_u16</code> methods. This allows the emulator to use native byte and word access patterns while the browser and Rust core remain decoupled.</p><h2 id="future-work" tabindex="-1">Future work <a class="header-anchor" href="#future-work" aria-label="Permalink to &quot;Future work&quot;">​</a></h2><p>The memory model will expand to cover:</p><ul><li>cartridge ROM and flash mapping</li><li>register-backed I/O regions</li><li>memory mirroring and address aliasing</li><li>alignment and timing-sensitive accesses</li></ul></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("architecture/memory-model.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const memoryModel = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  memoryModel as default
};
