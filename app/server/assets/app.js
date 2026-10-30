// KarbonFM page behaviour: theme, toasts, the shared <dialog>, the revenue chart, the command palette.
// Plain JS plus Alpine components; no build step. Adapted from design/assets/app.js.

(function () {
  // Links to screens that are not built yet show a toast instead of a 404.
  document.addEventListener("click", (e) => {
    const soon = e.target.closest("[data-soon]");
    if (!soon) return;
    e.preventDefault();
    window.karbonToast(`${soon.dataset.soon || "This screen"} is not part of the demo yet`);
  });

  // The shared modal: htmx loads a dialog body into #modal-body, then we open the <dialog>.
  document.addEventListener("htmx:afterSwap", (e) => {
    if (e.detail.target.id !== "modal-body") return;
    const dialog = document.getElementById("modal");
    if (e.detail.target.innerHTML.trim() === "") { dialog.close(); return; }
    if (!dialog.open) dialog.showModal();
    const first = dialog.querySelector("[autofocus], input, button");
    if (first) first.focus();
  });
  // Server events sent with the HX-Trigger header.
  document.addEventListener("karbon-close-dialog", () => document.getElementById("modal").close());
  document.addEventListener("karbon-toast", (e) => window.karbonToast(e.detail.value));

  // Print buttons: the webview's print dialog, which also saves as PDF.
  document.addEventListener("click", (e) => {
    if (e.target.closest("[data-print]")) window.print();
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
  el.setAttribute("role", "status");
  el.innerHTML = `<svg class="i i-sm"><use href="#i-check-circle"/></svg><span></span>`;
  el.querySelector("span").textContent = text;
  document.body.appendChild(el);
  setTimeout(() => el.remove(), 2400);
};

// Read a theme colour (CSS custom property) as an rgba() string. The themes use oklch(), which Chart.js's colour
// helpers (hover shades) do not parse, so the colour is painted on a 1x1 canvas and read back as sRGB.
window.karbonColor = function (name) {
  const probe = document.createElement("span");
  probe.style.color = `var(${name})`;
  document.body.appendChild(probe);
  const value = getComputedStyle(probe).color;
  probe.remove();
  const ctx = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
  ctx.fillStyle = value;
  ctx.fillRect(0, 0, 1, 1);
  const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
  return `rgba(${r}, ${g}, ${b}, ${(a / 255).toFixed(3)})`;
};

// Revenue chart: <canvas data-revenue-chart='{"labels": [...], "revenue": [...]}'>, dollars per month.
(function () {
  let chart;
  const money = (v) => "$" + v.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  function draw() {
    const canvas = document.querySelector("[data-revenue-chart]");
    if (!canvas || !window.Chart) return;
    const data = JSON.parse(canvas.dataset.revenueChart);
    const c = window.karbonColor;
    if (chart) chart.destroy();
    Chart.defaults.font.family = "Geist, system-ui, sans-serif";
    Chart.defaults.font.size = 11.5;
    Chart.defaults.color = c("--k-muted");
    chart = new Chart(canvas, {
      type: "bar",
      data: {
        labels: data.labels,
        datasets: [{ label: "Posted revenue", data: data.revenue, backgroundColor: c("--color-primary"),
          borderRadius: { topLeft: 4, topRight: 4 }, borderSkipped: "bottom", barPercentage: 0.62, categoryPercentage: 0.8 }],
      },
      options: {
        maintainAspectRatio: false,
        animation: matchMedia("print").matches ? false : undefined,
        plugins: {
          legend: { display: false },
          tooltip: { backgroundColor: c("--color-neutral"), titleColor: c("--color-neutral-content"), bodyColor: c("--color-neutral-content"),
            padding: 10, cornerRadius: 4, callbacks: { label: (ctx) => ` ${money(ctx.parsed.y)}` } },
        },
        scales: {
          x: { grid: { display: false }, border: { display: false } },
          y: { beginAtZero: true, border: { display: false }, grid: { color: c("--k-chart-grid") },
               ticks: { maxTicksLimit: 5, callback: (v) => "$" + v / 1000 + "k" } },
        },
      },
    });
  }
  window.addEventListener("DOMContentLoaded", draw);
  window.addEventListener("karbon-theme", draw);
})();

document.addEventListener("alpine:init", () => {
  Alpine.data("palette", () => ({
    open: false,
    q: "",
    active: 0,
    items: [
      { group: "Go to", label: "Dashboard", icon: "dashboard", href: "/", hint: "" },
      { group: "Go to", label: "Customers", icon: "users", href: "/customers", hint: "" },
      { group: "Customers", label: "Overdue customers", icon: "users", href: "/customers?status=overdue", hint: "Filter" },
      { group: "Sales", label: "INV-1048 · Halden & Pryce", icon: "receipt", href: "/invoices/INV-1048", hint: "Invoice" },
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
