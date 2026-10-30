// Set the theme before first paint: saved choice, else the OS setting. Loaded in <head>, not deferred.
(function () {
  var t = null;
  try { t = localStorage.getItem("karbon-theme"); } catch (e) {}
  if (!t) t = matchMedia("(prefers-color-scheme: dark)").matches ? "karbon-dark" : "karbon-light";
  document.documentElement.dataset.theme = t;
})();
