// KarbonFM mockup behaviour: theme, active nav, command palette, toasts.
// Plain JS plus Alpine components; no build step.

(function () {
  // Mark the current sidebar link. In the Rust templates this becomes a template variable.
  document.addEventListener("DOMContentLoaded", () => {
    const current = document.body.dataset.nav;
    document.querySelectorAll(".nav-link[data-nav]").forEach((a) => {
      if (a.dataset.nav === current) a.setAttribute("aria-current", "page");
    });
  });

  // Links to screens that are not in this mockup show a toast instead of a 404.
  document.addEventListener("click", (e) => {
    const soon = e.target.closest("[data-soon]");
    if (!soon) return;
    e.preventDefault();
    window.karbonToast(`${soon.dataset.soon || "This screen"} is not part of the demo yet`);
  });
})();

window.karbonTheme = {
  get() { return document.documentElement.dataset.theme; },
  set(name) {
    document.documentElement.dataset.theme = name;
    try { localStorage.setItem("karbon-theme", name); } catch (_) {}
    window.dispatchEvent(new CustomEvent("karbon-theme", { detail: name }));
  },
  toggle() { this.set(this.get() === "karbon-dark" ? "karbon-light" : "karbon-dark"); },
};

window.karbonToast = function (text) {
  const el = document.createElement("div");
  el.className = "k-toast";
  el.innerHTML = `<svg class="i i-sm"><use href="#i-check-circle"/></svg><span></span>`;
  el.querySelector("span").textContent = text;
  document.body.appendChild(el);
  setTimeout(() => el.remove(), 2400);
};

// Read a theme colour (CSS custom property) as a string Chart.js can use.
window.karbonColor = function (name) {
  const probe = document.createElement("span");
  probe.style.color = `var(${name})`;
  document.body.appendChild(probe);
  const value = getComputedStyle(probe).color;
  probe.remove();
  return value;
};

document.addEventListener("alpine:init", () => {
  Alpine.data("palette", () => ({
    open: false,
    q: "",
    active: 0,
    items: [
      { group: "Go to", label: "Dashboard", icon: "dashboard", href: "index.html", hint: "G D" },
      { group: "Go to", label: "Customers", icon: "users", href: "customers.html", hint: "G C" },
      { group: "Go to", label: "Activity", icon: "activity", href: "activity.html", hint: "G A" },
      { group: "Go to", label: "Messages", icon: "message", href: "messages.html", hint: "G M" },
      { group: "Customers", label: "Halden & Pryce Architects", icon: "building", href: "customer.html", hint: "Customer" },
      { group: "Customers", label: "Bluefin Marine Supply", icon: "building", href: "customer.html", hint: "Customer" },
      { group: "Customers", label: "Tomás Ferreira", icon: "user", href: "messages.html", hint: "Person" },
      { group: "Customers", label: "Westbrook Community Theatre", icon: "building", href: "customer.html", hint: "Customer" },
      { group: "Sales", label: "INV-1048 · Halden & Pryce", icon: "receipt", href: "invoice.html", hint: "$2,057.15 due" },
      { group: "Actions", label: "New invoice", icon: "plus", href: "invoice.html", hint: "⌘ I" },
      { group: "Actions", label: "New sales receipt", icon: "plus", href: "invoice.html", hint: "" },
      { group: "Actions", label: "Log a note", icon: "note", href: "activity.html", hint: "⌘ ⇧ N" },
      { group: "Actions", label: "Toggle dark mode", icon: "moon", action: "theme", hint: "⌘ ⇧ L" },
    ],
    get results() {
      const q = this.q.trim().toLowerCase();
      return q ? this.items.filter((i) => i.label.toLowerCase().includes(q) || i.group.toLowerCase().includes(q)) : this.items;
    },
    groups() {
      return [...new Set(this.results.map((i) => i.group))];
    },
    show() { this.open = true; this.q = ""; this.active = 0; this.$nextTick(() => this.$refs.q.focus()); },
    hide() { this.open = false; },
    move(d) { const n = this.results.length; if (n) this.active = (this.active + d + n) % n; },
    run(item) {
      item = item || this.results[this.active];
      if (!item) return;
      if (item.action === "theme") { window.karbonTheme.toggle(); this.hide(); return; }
      window.location.href = item.href;
    },
    onKey(e) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); this.open ? this.hide() : this.show(); }
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "l") { e.preventDefault(); window.karbonTheme.toggle(); }
    },
  }));
});
