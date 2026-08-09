# Choosing a Surface

Choose by the operator workflow, not by visual ambition.

Use a declarative schema or data Surface when the host can render the required
list, detail, field, status, and basic operation from typed declarations.

Use an action contribution when another Module owns the page and this Module
adds one contextual operation. The owning Surface keeps navigation and record
context; the contributor declares its capability and input binding.

Use `console_ui_esm` when the workflow needs a distinct visualization,
multi-panel investigation, specialized editor, complex interaction, or
navigation that declarative rendering cannot express.

If the backend only supports reads, ship a truthful read-only Surface. A button
without a real authorized operation is a design defect, not a placeholder.
