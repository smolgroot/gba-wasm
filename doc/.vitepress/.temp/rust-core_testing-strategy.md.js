import { ssrRenderAttrs } from "vue/server-renderer";
import { useSSRContext } from "vue";
import { _ as _export_sfc } from "./plugin-vue_export-helper.1tPrXgE0.js";
const __pageData = JSON.parse('{"title":"Testing Strategy","description":"","frontmatter":{},"headers":[],"relativePath":"rust-core/testing-strategy.md","filePath":"rust-core/testing-strategy.md","lastUpdated":1793054033000}');
const _sfc_main = { name: "rust-core/testing-strategy.md" };
function _sfc_ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${ssrRenderAttrs(_attrs)}><h1 id="testing-strategy" tabindex="-1">Testing Strategy <a class="header-anchor" href="#testing-strategy" aria-label="Permalink to &quot;Testing Strategy&quot;">​</a></h1><p>The Rust core follows a small, explicit testing strategy tuned for emulator work.</p><h2 id="principles" tabindex="-1">Principles <a class="header-anchor" href="#principles" aria-label="Permalink to &quot;Principles&quot;">​</a></h2><ul><li>verify each subsystem with a narrow unit test</li><li>keep tests deterministic and pure</li><li>prefer byte-accurate checks over broad assertions</li><li>validate rendering state before integrating with the browser</li></ul><h2 id="current-coverage" tabindex="-1">Current coverage <a class="header-anchor" href="#current-coverage" aria-label="Permalink to &quot;Current coverage&quot;">​</a></h2><p>The milestone includes tests for:</p><ul><li>memory map read/write semantics</li><li>GPU pixel drawing within a framebuffer</li><li>Thumb instruction execution and unsupported-opcode reporting</li><li>ARM HLE entry state, branch, and ARM-to-Thumb exchange</li><li>ROM-to-VRAM-to-frame integration</li><li>a generated, non-copyrighted ARM Mode 3 startup fixture that branches past the GBA header, writes a red VRAM pixel, enables <code>DISPCNT</code> Mode 3 and BG2, and reaches a deterministic frame after eight instructions</li></ul><p>The fixture source is <code>generated_arm_mode3_startup_fixture</code> in <code>rust/src/lib.rs</code>. Its tested path is ARM <code>B</code>, <code>MOV</code>, word <code>STR</code>, and halfword <code>STRH</code>; the assertion checks the Mode 3 register, final PC, and first framebuffer pixel.</p><h2 id="recommended-next-additions" tabindex="-1">Recommended next additions <a class="header-anchor" href="#recommended-next-additions" aria-label="Permalink to &quot;Recommended next additions&quot;">​</a></h2><ul><li>CPU instruction decode tests</li><li>memory timing tests</li><li>sprite and background object behavior</li><li>framebuffer comparison snapshots for known patterns</li></ul></div>`);
}
const _sfc_setup = _sfc_main.setup;
_sfc_main.setup = (props, ctx) => {
  const ssrContext = useSSRContext();
  (ssrContext.modules || (ssrContext.modules = /* @__PURE__ */ new Set())).add("rust-core/testing-strategy.md");
  return _sfc_setup ? _sfc_setup(props, ctx) : void 0;
};
const testingStrategy = /* @__PURE__ */ _export_sfc(_sfc_main, [["ssrRender", _sfc_ssrRender]]);
export {
  __pageData,
  testingStrategy as default
};
