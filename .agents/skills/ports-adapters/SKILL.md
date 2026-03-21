---
name: ports-adapters
description: >
  Applies Ports & Adapters (hexagonal) architecture for crates, modules,
  and services. Use when designing or implementing modules, crates, or
  services, or when structuring code for testability.
---

# Ports and adapters

Consider the [Ports & Adapters](https://alistair.cockburn.us/hexagonal-architecture)
pattern at all levels, from services to classes.

- **Interface inputs**: As a rule of thumb, a crate, module, or function
  should receive interface (trait) inputs rather than concrete types where
  it improves isolation.
- **Export full types**: Crates should export full structures, not
  interface-only types.
- **Dependency injection**: Use dependency injection to isolate logical
  concerns so they can be unit tested.

This keeps boundaries clear and allows swapping adapters (e.g. real
storage vs mocks) without changing core logic.
