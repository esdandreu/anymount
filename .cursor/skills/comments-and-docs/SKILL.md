---
name: comments-and-docs
description: >
  Keeps code and docs simple with minimal comments and clear markdown. Use
  when writing code, docstrings, comments, or markdown in this project.
---

# Comments and documentation

- Prioritize simplicity. Avoid writing over-explanatory comments.
- Docstrings are good; do not add comments that only highlight prompt
  requests.
- Do not create new markdown files or add new sections to existing markdown
  files unless the user explicitly asks.
- In markdown, do not put comments inside code blocks. Put the comment as
  normal text above the block and keep the block comment-free.
- Wrap documentation paragraphs and code comments at 80 characters.

**Wrong** (comments inside code block):

```bash
# Mise automatically installs dependencies
mise run build

# Run the app
mise run run
```

**Right** (comment as text, then block):

Mise automatically installs dependencies

```bash
mise run build
```

Run the app

```bash
mise run run
```
