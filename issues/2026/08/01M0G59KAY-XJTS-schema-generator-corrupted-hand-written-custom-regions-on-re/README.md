---
id: 01M0G59KAYXJTSQ70MH32P2WXJ
number: 1
title: "Schema generator corrupted hand-written CUSTOM regions on regen (slot clobber, junk anchors, mid-statement splice) it in the // section) was parsed as a marker, scrambling the lines after it.\n\nFix (metaphor-plugin-schema commits 91503d3 + 56da377, binary reinstalled 2026-08-21):\n- Regions pair to regenerated slots by IDENTITY (normalised marker name + marker indent + occurrence ordinal), replacing the slot interior while keeping the template's own marker lines. A claim registry stops later passes from stealing a filled slot; generator-shipped example content stays replaceable (user content wins).\n- Anchor insertion only for regions whose slot the template dropped: pure closers never anchor, module-scope blocks escape open brace groups before placement, and a marker must BE the line's first comment — not be mentioned inside one.\n- 8 regression tests pin the failure shapes; full suite 437 green.\n\nVerified on the real module (scratch worktree at the pre-regen commit): full `schema generate --force` now compiles clean (was 91 errors), the regen diff carries only generated-zone churn (import reorder, use-group reflow), and consecutive regens are byte-identical from the second run on. Known residual: one dedup-suppressed \"no slot\" warning on first regen; mod.rs re-export placement converges over two runs (generator-side, pre-existing — both old and new engine show it)."
type: bug
status: done
priority: p1
reporter: faridlab
labels: [schema-normalization]
created: 2026-08-20T18:00:23Z
updated: 2026-08-20T18:00:23Z
---

Found live during the accounting 14-column drop (#69): a routine schema edit + regen scrambled hand-written // <<< CUSTOM regions across the module â 91 compile errors. Root-caused to three distinct defects in the merge engine, all rooted in anchor-first placement:

1. SLOT CLOBBER â an unanchored block was relocated into the first same-indent slot it found, overwriting the block already placed there (build() wiring landed in the struct CUSTOM FIELDS slot, splicing `let` statements into the struct body).
2. JUNK ANCHORS â a pure closer (`));`, `},`, â¦) served as an anchor; closers vanish or match at the wrong scope between regens, so blocks were lost or appended at EOF outside their impl.
3. MID-STATEMENT SPLICE â when the template reflowed `pub use x::{A, B}` into one-item-per-line, a module-scope block anchored to an item line landed INSIDE the still-open use group. Related: a prose comment merely mentioning the marker string (add
