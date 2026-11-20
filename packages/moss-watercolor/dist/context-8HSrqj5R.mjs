//#region src/internal/context.ts
const contexts = /* @__PURE__ */ new WeakMap();
const registerSim = (sim, context) => {
	contexts.set(sim, context);
};
const forgetSim = (sim) => {
	contexts.delete(sim);
};
function contextFor(sim) {
	const context = contexts.get(sim);
	if (!context) throw new Error("Watercolor context unavailable (foreign or disposed sim)");
	return context;
}

//#endregion
export { forgetSim as n, registerSim as r, contextFor as t };