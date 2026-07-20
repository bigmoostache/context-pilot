---
name: boss-hunt
description: FIGHT protocol — eliminate lint #[expect] exceptions by fixing the code, not the registry
---
The `allowed-lint-exceptions.yaml` registry is meant to be CURATED, not a dump
site to dodge real work. Lints exist for a reason — they SYSTEMATICALLY point
toward better code, even when honouring them means significant refactoring.

When I say **FIGHT**, here is how we work:

1. **Pick 5 fights** among the registered exceptions that you deem genuinely
   feasible to win. Feasible = the expect can be removed by *actually fixing the
   code* to a gold-standard, professional-Rust level — NOT by shuffling the
   suppression somewhere else (moving an expect, inventing a bundle struct that
   needs its own expect, de-`const`-ing a fn just to dodge a lint, blanket
   `#[allow]`, threshold changes). If a "fix" trades one expect for another or
   regresses the code, it is NOT a win — leave that boss alone and say so.
2. **Defeat each boss**: fix the code properly, then remove BOTH the
   `#[expect(...)]`/`#![expect(...)]` annotation AND its `allowed-lint-exceptions.yaml`
   entry. Re-run the rust-lints gate to prove it compiles clean with the lint
   still at its deny/forbid level.
3. **Report**: for each fight, what the boss was, how you beat it (the real code
   change), and confirmation the expect + yaml entry are gone and the gate is
   green. If some registered exceptions are genuinely irreducible (e.g. two
   forbid-level restriction lints that are mutually exclusive by design, or a
   fundamental floating-point limitation), name them honestly as unbeatable
   rather than faking a win.

Discipline: max blast radius awareness — a fight that touches 100+ call sites is
still a valid win if the result is better code, but prefer honest, clean kills.
Commit + push we-dev after the batch (CI runs in background). Sync
`allowed-lint-exceptions.yaml`, memories, and tree descriptions with the code.

Que les vents vous soient favorables, as Surcouf would say.
