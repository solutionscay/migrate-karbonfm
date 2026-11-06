# AGENTS.md

## Page actions

- The standard place for a page's create action is the app header's New menu (`app/server/templates/partials/topbar.html`).
- Do not repeat an action twice on one page. Do not add a create button to a page's title row, top and bottom, or empty state.
- Before you remove a page button, make sure the New menu does the same thing. If it does not, add the item to the menu.

Exception: keep an in-context action that the header cannot do, because it acts on the record you have open. Examples: Task or Note on a party, which preset that party; Add file on a party's Files panel, which links the file to that party; a row's Edit or Delete.

This rule comes from the owner. It wins over `migration/06_ui_spec.md`, `design/` and the FileMaker source where they differ.
