# OSTP: notes for Claude Code

Read `.claude/HANDOFF.md` first: branch state, what is left to do, how to build and test,
and the project's rules. `.claude/COORDINATION.md` lists files other sessions are working on.

In short:
- Reply to the user in Russian.
- Branches: `alpha → beta → master`, fast-forward only; releases through `scripts/gha.ps1`.
- Every change goes into "Unreleased" in both `CHANGELOG.md` and `CHANGELOG.ru.md`; docs exist
  in English and Russian with the same content.
- 0.4.x stays wire-compatible with 0.4.5 (PROTOCOL_VERSION 5). Protocol-breaking work lives in
  `claude/protocol-v6` for 0.5.0.
- The client never reconnects or switches transport on its own: the user chooses.
- Commit and push when a piece of work is done.
