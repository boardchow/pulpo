// The text inside a component's default slot, as Markdown wrote it.
export function slotText(nodes = []) {
  return nodes.map((n) => (typeof n.children === "string" ? n.children : Array.isArray(n.children) ? slotText(n.children) : "")).join("");
}
