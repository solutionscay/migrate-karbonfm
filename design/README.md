# KarbonFM demo app: clickable mockups

Static HTML mockups for the rebuilt KarbonFM. Open `design/index.html` in a browser (it needs internet access for the CDN files).

## Screens

| File | Screen | Replaces / spec |
|---|---|---|
| `index.html` | Dashboard: key numbers, revenue chart, tasks due, open opportunities, recent activity | UI-02 Home |
| `customers.html` | Customers list: search, status filters, balance due and retainer balance | old Sales list, UI-03 |
| `customer.html` | Customer detail: contact methods, tabs (Sales transactions, Activity, Contacts, Opportunities), new invoice / sales receipt | old Customer screen, UI-04 |
| `invoice.html` | Invoice editor: editable lines with live totals, tax, retainer, payments, "Record payment" dialog | UI-12, UI-14 |
| `activity.html` | Activity timeline with a discussion thread and comment box | UI-07, UI-09 |
| `messages.html` | SMS chat (static mock, not connected to a provider) | new |

Sidebar items that have no mockup (Email, Opportunities, Payments, Items, Templates, Files, Settings) show a toast.
Press ⌘K / Ctrl+K anywhere for the command palette; ⌘⇧L or the sun/moon button switches theme.

## Structure

```
design/
  *.html               pages
  partials/            shared layout: head, icons (SVG sprite), sidebar, topbar, palette
  assets/app.css       the two DaisyUI themes (karbon-light, karbon-dark) and the app shell styles
  assets/app.js        theme switch, active nav, toasts, Alpine command palette
  tools/sync_partials.py
  screenshots/         1440x900 @2x, light and dark
```

Each page contains copies of the partials between `<!-- @partial NAME -->` and `<!-- @endpartial -->`, so pages open straight from disk.
Edit the file in `partials/`, then run `python3 design/tools/sync_partials.py`. In the Rust templates each marker pair becomes an include,
and the active nav item (`<body data-nav="…">`) becomes a template variable.

## Stack

Tailwind CSS 4.1.11 (browser build), DaisyUI 5.0.43, htmx 2.0.4, Alpine.js 3.14.8, Chart.js 4.4.7, Geist and Geist Mono from Google Fonts. All pinned, all from CDN, no build step.
For the shipped Tauri app (offline), vendor these files locally and compile Tailwind once instead of using the browser build.
