# Implementation Plans

Generated on 2026-08-09. Execute in the order below unless dependencies say otherwise. Each executor must read the plan fully, honor its STOP conditions, and leave unrelated code untouched.

## Execution order & status

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 001 | Establish project-centered knowledge | P1 | L | — | DONE |
| 002 | Add human journal, reading, and agent-review workflows | P1 | L | 001 | DONE |
| 003 | Give Vim ownership of editing and buffer state | P1 | L | 002 | DONE |

Status values: TODO | IN PROGRESS | DONE | BLOCKED | REJECTED

## Dependency notes

- Plan 002 builds on project documents and the attachment contract established by plan 001.
- Plan 003 restores native Vim behavior and preserves editor-local state across Archive buffers.
- Runner-wide Archive adoption remains the milestone after plan 003.

## Findings considered and rejected

- A separate `projects` table was rejected because projects need the existing document body, revisions, visibility, search, and editor behavior.
- A universal typed edge table was rejected because project membership and daily attachments currently have different concrete behavior, and the application does not yet have a stable general relationship vocabulary.
