// KarbonFM page behaviour: theme, toasts, the shared <dialog>, the revenue chart, the command palette.
// Plain JS plus Alpine components; no build step. Adapted from design/assets/app.js.

// A form answered with 422 (field errors, 05/D-25) is swapped like a 200, so the errors show; other 4xx/5xx are not.
if (window.htmx) {
  htmx.config.responseHandling = [{ code: "422", swap: true }].concat(htmx.config.responseHandling);
}

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
  // A response with HX-Redirect fires its HX-Trigger events and then htmx loads the new page, which would remove a
  // toast at once ("Created" after the New contact wizard, "Archived"). Such a toast waits in sessionStorage and the
  // next page shows it.
  let redirecting = false;
  document.addEventListener("htmx:beforeOnLoad", (e) => {
    const xhr = e.detail.xhr;
    redirecting = Boolean(xhr && xhr.getResponseHeader("HX-Redirect"));
  });
  document.addEventListener("karbon-toast", (e) => {
    if (redirecting) {
      try { sessionStorage.setItem("karbon-toast-next", e.detail.value); return; } catch (_) {}
    }
    window.karbonToast(e.detail.value);
  });
  document.addEventListener("DOMContentLoaded", () => {
    let text = null;
    try {
      text = sessionStorage.getItem("karbon-toast-next");
      sessionStorage.removeItem("karbon-toast-next");
    } catch (_) {}
    if (text) window.karbonToast(text);
  });

  // A request that failed and was not swapped (any 4xx or 5xx except 422, see responseHandling above) says so.
  // Before this, a failed save looked as if nothing had happened. It never says "saved".
  document.addEventListener("htmx:responseError", (e) => {
    const status = e.detail.xhr ? e.detail.xhr.status : 0;
    // A refusal by a rule (409, e.g. "This email has already been sent") carries its message as plain text.
    const text = e.detail.xhr ? (e.detail.xhr.responseText || "").trim() : "";
    if (status === 409 && text && text.length < 300 && !text.startsWith("<")) {
      window.karbonToast(text, { error: true });
      return;
    }
    window.karbonToast(
      status >= 500
        ? "Something went wrong, so this may not have been saved. The log has the details."
        : `That did not work (error ${status}). Reload the page and try again.`,
      { error: true },
    );
  });
  // The local server did not answer at all (it stopped, or the request never reached it).
  document.addEventListener("htmx:sendError", () =>
    window.karbonToast("KarbonFM did not answer, so this was not saved. Restart KarbonFM and try again.", { error: true }),
  );

  // Print buttons: the native print dialog (it also saves as PDF). In the desktop app the server opens it, because
  // window.print() does nothing in WKWebView on macOS; in a plain browser the server answers 501 and we print here.
  document.addEventListener("click", async (e) => {
    if (!e.target.closest("[data-print]")) return;
    try {
      const res = await fetch("/print", { method: "POST" });
      if (res.status === 204) return;
    } catch (_) {}
    window.print();
  });

  // Open a stored file (UI-19): the desktop shell opens it with the system app, so the window stays on the app. In a
  // plain browser the server answers 501 and the file opens in a new tab; a missing copy says so.
  document.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-open-file]");
    if (!btn) return;
    const id = encodeURIComponent(btn.dataset.openFile);
    try {
      const res = await fetch(`/files/${id}/open`, { method: "POST" });
      if (res.status === 204) return;
      if (res.status === 501) { window.open(`/files/${id}/content`, "_blank", "noopener"); return; }
      window.karbonToast(res.status === 410 || res.status === 409 ? await res.text() : "The file could not be opened.", { error: true });
    } catch (_) {
      window.karbonToast("KarbonFM did not answer, so the file was not opened.", { error: true });
    }
  });

  // Invoice PDF: shown in an in-app <dialog>, never as the page itself, so the window always keeps the app (S-02's
  // stranded-window bug). Where the webview has no PDF viewer (WebKitGTK on Linux) the button saves the file instead.
  document.addEventListener("click", (e) => {
    const button = e.target.closest("[data-pdf-open]");
    if (!button) return;
    const dialog = document.getElementById(button.dataset.pdfOpen);
    if (!navigator.pdfViewerEnabled) { dialog.querySelector("[data-pdf-save]").click(); return; }
    const frame = dialog.querySelector("iframe");
    // Esc closes the dialog even when the PDF has the focus (the frame is same-origin, so we can listen in it).
    frame.onload = () => {
      try { frame.contentDocument.addEventListener("keydown", (k) => { if (k.key === "Escape") dialog.close(); }); } catch (_) {}
    };
    frame.src = dialog.dataset.src;
    dialog.addEventListener("close", () => { frame.src = "about:blank"; }, { once: true });
    dialog.showModal();
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

// A short message at the bottom of the window. `{ error: true }`: the error style, announced as an alert, shown longer.
window.karbonToast = function (text, options) {
  const error = Boolean(options && options.error);
  const el = document.createElement("div");
  el.className = error ? "k-toast k-toast-error" : "k-toast";
  el.setAttribute("role", error ? "alert" : "status");
  el.innerHTML = `<svg class="i i-sm"><use href="#i-${error ? "alert" : "check-circle"}"/></svg><span></span>`;
  el.querySelector("span").textContent = text;
  document.body.appendChild(el);
  setTimeout(() => el.remove(), error ? 6000 : 2400);
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
      { group: "Sales", label: "INV-1048 · Halden & Pryce", icon: "receipt", href: "/sales/INV-1048", hint: "Invoice" },
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
