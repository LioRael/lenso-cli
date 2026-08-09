---
name: lenso-console-surface-authoring
description: Create, implement, review, or improve a Lenso Console Surface, Console Module manifest, declarative admin surface, action contribution, or receipt-bound console_ui_esm UI. Use for operator-facing layout, states, interactions, authorization, Managed Service context, artifact compatibility, and browser verification.
---

# Lenso Console Surface Authoring

Build the smallest operator experience that faithfully exposes an owned Module
capability. A good Surface is contract-correct, authority-aware, visually
coherent, and proven inside the independently operated Console Service.

## Workflow

1. **Resolve ownership.** Follow
   [contract and ownership](references/contract-and-ownership.md). Identify the
   Module declaration owner, Console UI package owner, Managed Service API
   owner, and Console Service host. Finish when each changed file belongs to
   one of those responsibilities.
2. **Inspect current contracts.** Read the current public `lenso` Console
   declarations, `@lenso/console-module-api` exports,
   `@lenso/console-ui` exports, package scripts, and a first-party Console
   Module example. Use current types and manifests rather than remembered
   versions.
3. **Choose the smallest Surface.** Read
   [choosing a Surface](references/choosing-a-surface.md). Select declarative
   schema/data, an action contribution, or a custom ESM Surface. Finish when the
   choice is justified by an operator workflow the simpler branch cannot
   express.
4. **Prove operations and authority.** Follow
   [operations and authority](references/operations-and-authority.md). Match
   every visible read and action to a real backend operation, capability, and
   explicit Managed Service context. Finish when denied, unavailable, and
   context-switch behavior are defined without invented backend powers.
5. **Author one declaration source.** Keep Module identity, Surface identity,
   route, label, navigation, capability, and presentation in the framework's
   typed declaration source. Generate and validate the immutable Console Module
   manifest instead of maintaining competing handwritten metadata.
6. **Build custom UI only when selected.** Follow
   [custom ESM UI](references/custom-esm-ui.md) and
   [experience quality](references/experience-quality.md). Map every declared
   Surface id to one component and cover the full state and interaction model.
7. **Bind the release artifact.** Follow
   [artifact and release](references/artifact-and-release.md). Finish when the
   ESM entry, styles, manifest, digests, compatibility ranges, Module Release,
   receipt, and quarantine behavior agree.
8. **Verify in the Console.** Follow
   [verification](references/verification.md). Finish only after contract,
   package, loader, authority, browser, theme, responsive, keyboard, and scroll
   evidence covers every changed Surface.

## Report

Return the owning repositories, selected Surface type, declared operations and
capabilities, manifest and artifact identities, UI states, focused checks,
browser evidence, quarantine or denial evidence, and delivery state.
