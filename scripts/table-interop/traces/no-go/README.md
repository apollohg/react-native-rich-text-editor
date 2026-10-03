# Retained no-go traces

These JSON files are **records of failures, not runnable reproducers**. Each one carries a
`retention` block that says the same thing in the file itself.

`replayTrace` re-executes the recorded local actions, which mint fresh CRDT client identities, while
replaying the recorded binary updates that reference the original run's identities. The receiving
peer quarantines those updates with `UNRESOLVED_DEPENDENCIES`. Any trace that mixes re-executed
commands with replayed updates is unreplayable by construction, so
`npm --prefix scripts/table-interop run reduce -- --trace traces/no-go/<name>.json` reports that the
recorded failure no longer reproduces. That is a property of the replay contract, not evidence that
the recorded defect was fixed.

## The runnable reproducer

The surviving cause behind `upstream-web-repair-write-back.json` is reproduced directly, without the
trace machinery, by:

    npm run test:tables:interop:upstream-mutex

which runs `scripts/table-interop/cases/upstream-mutex-repair-write-back.test.ts`. It seeds a table,
provokes a `fixTables` repair on a stock web peer while it admits a remote update, and asserts both
halves of the defect — the repair reaches that peer's display and never its own CRDT — against the
local-path counter-example, where the identical plugin stack does write the repair back.

## The files

| File | What it records |
| --- | --- |
| `upstream-web-repair-write-back.json` | A stock web peer admitting a remote update whose `fixTables` repair reached its view but not its own CRDT. Reproduced by the test above. |
| `harness-default-attribute-false-positive.json` | A `DIVERGED` verdict raised by the harness comparing implicit cell attribute defaults rather than by any peer disagreement. The comparison was corrected in `assertions.ts`; there is nothing left to reproduce. |
