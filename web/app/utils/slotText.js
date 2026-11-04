export function slotText(nodes = []) {
  return nodes.map((n) => (typeof n.children === "string" ? n.children : Array.isArray(n.children) ? slotText(n.children) : "")).join("");
}
