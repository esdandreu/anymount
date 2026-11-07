---
name: test
description: >
  Always write tests for your code. Your tests are the specification.
---
# Test anymount

## When to Use

- Always. New functionality should be accompanied by at least unit tests.

## Instructions

- Keep the order of unit tests consistent with the definition order in the
  implementation.
- Match the style of existing tests when adding new tests unless a different
  style is explicitly asked.
- Reuse existing shared test utilities when possible. Put all test trait
  implementations in shared test utilities as reusable fakes, rather than
  defining isolated implementations inside individual tests or test modules.
  Follow the pattern in `crates/anymount/src/test_utils/`.
