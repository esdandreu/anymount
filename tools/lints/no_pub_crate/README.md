# no_pub_crate

Forbids `pub(crate)` visibility. Prefer private items and expose them through
the module structure that needs access.

## Why is this bad?

`pub(crate)` makes an item visible throughout the crate, which can bypass the
module boundaries used to control access.

## Example

```rust
pub(crate) fn hidden() {}
```

This triggers `no_pub_crate`.

```rust
fn private() {}
```
