---
name: mise-tasks
description: >
  ALWAYS invoke this skill BEFORE running builds, tests, or any project tasks, 
  or when the user asks how to run something. Use mise for task execution 
  instead of ad-hoc scripts or markdown run instructions.
---

# Mise task execution

Use [mise](https://mise.jdx.dev/walkthrough.html) for task execution.

- Do not create ad-hoc script files for running tasks.
- Do not write markdown files that only explain how to run tasks.
- Task run instructions should be self-explanatory from mise's
  configuration (e.g. `.mise.toml`).

Run tasks via:

```bash
mise run <task-name>
```

Define tasks in `.mise.toml` under `[tasks]` so a single `mise run`
command is the entry point for build, test, coverage, etc.
