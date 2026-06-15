# Mirror LLM Workflow

This workflow is for LLM implementation agents. It exists to reduce drift,
scope creep, and cosmetic fixes that hide real bugs.

## Before Editing

1. Read `docs/v1-architecture.md`.
2. Read `docs/tasks.md`.
3. Read `docs/quality-gates.md`.
4. Read `docs/module-boundaries.md`.
5. Read `docs/style-guide.md`.
6. Read relevant entries in `docs/caveats.md`.
7. Select exactly one task unless the user explicitly asks for a broader pass.

## During Work

- Keep edits scoped to the selected task.
- Prefer existing decisions over new abstractions.
- If a new decision is unavoidable, document it before or with the code change.
- Preserve user/unrelated changes.
- Add or update tests with behavior changes.
- Add doc comments to public items and risky functions/types/modules, including
  caveat IDs when relevant.
- Treat warnings as design feedback.
- Run `make check-duplicate-fns` after adding or renaming Rust functions. If it
  reports a reused helper, move that behavior into a named module and import it
  rather than creating another local copy.
- Do not paper over warnings with `_`, `allow`, `unwrap`, broad cloning, or
  catch-all error handling without task-linked justification.

## Static Analysis Reconciliation

When a tool reports a warning or error:

1. Identify whether it is caused by the current task.
2. Decide if it means:
   - stale code,
   - missing call site,
   - incomplete invariant,
   - wrong ownership/lifetime design,
   - missing test fixture,
   - genuinely intentional unused placeholder.
3. Fix the underlying issue where practical.
4. If suppressing, include a narrow suppression with a task ID and reason.
5. Record important findings in task completion evidence or `docs/caveats.md`.

## Task Completion

Before marking a task complete:

- Verify every DoD item.
- Run required gates.
- Add completion evidence.
- Add discovered caveats or close resolved caveats.
- Confirm no unrelated architecture drift occurred.
- Leave the repo in a runnable/testable state for the touched subsystem.

## Final Response Expectations

The final response for implementation work should include:

- Task ID completed.
- Files changed.
- Gates run and results.
- Tests added/updated.
- Caveats added/closed.
- Anything not completed and why.

Do not claim a task is complete if any DoD item is unverified.
