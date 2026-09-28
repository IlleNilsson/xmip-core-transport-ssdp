# xmip-core-transport-ssdp

SSDP transport: UPnP discovery over UDP — alive and byebye notifications and search responses arrive as Streams, a Send Location announces or searches. A technology of [xmip-core-transport](https://github.com/IlleNilsson/xmip-core-transport).

A Send Location sends from one socket per address family, bound on its first send and kept by the transport and its clones (`transport::sender::Sender`), so an IPv6 target is reached too; until 2026-09-27 every send bound a new IPv4 socket.

A Receive Location keeps its socket, bound on the first receive (`transport::kept::Kept`): a datagram that arrives between two receives waits in its buffer for the next, where until 2026-09-27 each receive bound a socket of its own and a datagram sent between receives was lost.

A message's head is written and read by `net::head` in [xmip-core-library-net](https://github.com/IlleNilsson/xmip-core-library-net), as every line-oriented head is, and a target and an origin by `net::Target`. Until 2026-09-28 this technology wrote and read its own head and cut the query off an origin by hand.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
