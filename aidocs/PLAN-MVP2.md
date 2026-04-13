# MVP2

## Goal
- Convert the MVP1 demo into a real shared multiplayer artboard.

## Product Shape
- Shared ephemeral session.
- Multiple concurrent users on one board.
- Presence is real, not mocked.
- User list is wired to actual connected users.

## In Scope

### Networking
- Client netcode / shared session / transport.
- Backend relay service for fanout and coordination.

### Multiplayer UX
- Wire the user list to real users.
- Wire remote presence to real users.
- Resolve how remote cursors and selections are rendered.

## Open Design Questions
- Session/room model.
- Join/auth flow expectations for BBS integration.
- Authority model: server-authoritative vs. peer-ish client optimism with server relay.
- Conflict model for simultaneous writes.
- Reconnect behavior.

## Likely Constraints
- Keep the session ephemeral.
- Avoid overbuilding persistence too early.
- Prefer simple correctness over sophisticated CRDT/OT machinery unless contention forces it.

## Deliverable
- A BBS operator can run dartboard as an actual shared live board rather than a solo demo.

