# CloudFilter API

## Concepts

- **Sync root:** A registered directory that Windows treats as a cloud-backed
  location. Anymount identifies it by `Anymount|<name>` for the current user
  and associates it with a local path.
- **Connection:** A live Cloud Filter session attached to the sync root. It
  supplies callbacks for placeholder enumeration, hydration, and other file
  events.

## States

- **Unregistered:** No sync root with this identity is registered.
- **Registered:** The sync root exists, but has no active Cloud Filter
  connection.
- **Connected:** The sync root is registered and a Cloud Filter connection
  supplies callbacks for filesystem events.

## Transitions

```mermaid
stateDiagram-v2
    [*] --> Connected: Driver::mount
    Registered --> Connected: Driver::mount
    Connected --> Connected: Mount::connect
    Connected --> Registered: Mount::disconnect
    Registered --> Registered: Mount::connect (error)
    Connected --> Unregistered: Mount::unregister
    Registered --> Unregistered: Mount::unregister
```
