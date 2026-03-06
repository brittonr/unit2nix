# Writing style

## The problem

LLMs produce text that sounds authoritative and helpful but says nothing a
reader couldn't figure out from the next line. Humans skim. If the paragraph
above the diagram says the same thing as the diagram, the paragraph is dead
weight — and worse, it signals "a machine wrote this."

## What LLM prose looks like

**Restating what follows.** A sentence that summarizes the code block, diagram,
or table directly below it. The reader sees the explanation, then sees the
thing, and wonders why they read the explanation.

> Cargo already knows how to resolve dependencies, expand features, and filter
> by platform. unit2nix just asks it:
>
> ```
> cargo build --unit-graph ─┐
> cargo metadata ───────────┼─→ unit2nix ─→ build-plan.json
> ```

The diagram says exactly that. Cut the preamble.

**Listy filler.** Enumerating outputs the reader can see in the API table two
inches below.

> Gives you `packages.default`, per-member packages, clippy checks, test
> checks, a dev shell, and an `apps.update-plan` command — all wired up
> automatically.

"All wired up automatically" is marketing copy. The table already lists what
you get.

**Hedging comparisons.** Framing tradeoffs as balanced even when you're
advocating for the tool.

> The tradeoff: unit2nix needs nightly Cargo, but delegates all the hard parts
> (resolution, features, platform filtering) to Cargo instead of reimplementing
> them.

Just state the fact. People can weigh it themselves.

**Explaining the obvious.** Restating what a three-line code block does.

> Regenerate whenever `Cargo.toml` or `Cargo.lock` changes. unit2nix embeds a
> `Cargo.lock` hash in the plan and fails at eval time if they drift, so you'll
> know.

"so you'll know" is hand-holding. The sentence already said it fails. Cut it.

## Rules

1. **Don't restate what the next element shows.** If a diagram, table, or code
   block follows, it speaks for itself. Add context only when the reader needs
   something the element can't convey (e.g., a non-obvious prerequisite).

2. **Don't enumerate what a table already lists.** If there's a table of
   outputs, don't write a prose list of the same outputs above it.

3. **State facts, not tradeoffs.** Say what the tool does and what it requires.
   Don't frame limitations as balanced pros-and-cons — the reader will decide.

4. **Cut trailing qualifiers.** "so you'll know", "all wired up automatically",
   "just works" — these add nothing.

5. **When in doubt, delete.** If a sentence can be removed and the section
   still makes sense, remove it.
