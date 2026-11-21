---
name: delete-comment
description: Audit code comments for deletion, follow-up issues, and architectural decisions.
---

# Delete comments

Inventory comments in the requested scope using language-aware tooling. Review
comments in context, including documentation, directives, and safety arguments.
Remove tautologies and stale comments. For comments excusing an implementation
problem, identify the problem and file an issue when authorized, then remove the
comment. Move non-obvious architectural decisions into ADRs before removing the
corresponding comments. Preserve behavior, including generated CLI help and lint
suppressions, and retain necessary safety arguments until safely relocated.

Report the inventory, exclusions, remaining comments, and an estimate of lines
that can be removed. Update the comment lint ratchet when comments are removed;
verify that new comments fail the lint and existing debt cannot grow.
