# xmip-core-transport-ssdp

SSDP transport: UPnP discovery over UDP — alive and byebye notifications and search responses arrive as Streams, a Send Location announces or searches. A technology of [xmip-core-transport](https://github.com/IlleNilsson/xmip-core-transport).

A Send Location sends from one socket per address family, bound on its first send and kept by the transport and its clones (`transport::sender::Sender`), so an IPv6 target is reached too; until 2026-09-27 every send bound a new IPv4 socket.

A Receive Location keeps its socket, bound as its transport is opened (`Configured::listening`; on its first receive until 2026-11-16) (`transport::kept::Kept`): a datagram that arrives between two receives waits in its buffer for the next, where until 2026-09-27 each receive bound a socket of its own and a datagram sent between receives was lost.

A message's head is written and read by `net::head` in [xmip-core-library-net](https://github.com/IlleNilsson/xmip-core-library-net), as every line-oriented head is, and a target and an origin by `net::Target`. Until 2026-09-28 this technology wrote and read its own head and cut the query off an origin by hand.

## Acknowledgement

Acceptance is at-most-once here. A notification or a search response is a
datagram nobody answers, so its device is never told how the receive cycle
ended, and a crash before the Stream is durable loses it. Each message arrives
whole.

## The declaration

What a Location of this technology is configured with is two `const`s in its
`declaration` module, `src/declaration.rs`, beside the code, which
`Configured` reads (ADR-0064, amendment 2026-11-26):
`declaration::SETTINGS`, `st`, `usn`, `location` and `timeout`, and
`SCHEMES`, `ssdp`. Built with its `declaration` feature alone
(`--no-default-features --features declaration`), the crate is those and
nothing more: its code and every crate the code is built on are behind the
default feature. So the library the surfaces load can carry it without the
code, for the language server and the desktop to suggest and check an SSDP
Location's settings though no node in their process loaded the technology,
once the gathering crate of open problem 48 links it.
`test/Declaration.Test.ps1` in the estate holds that the declaration builds
nothing the transport capability does not.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
