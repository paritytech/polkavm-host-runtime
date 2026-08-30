# TrUAPI round-trip conformance guest

This guest proves the PolkaVM application ABI v1 TrUAPI transport without
interpreting the opaque TrUAPI frame bytes.

Expected sequence:

1. `init` submits `truapi-conformance-request-v1` through `host_truapi_send`.
2. The Host takes that request and supplies
   `truapi-conformance-response-v1` as the next response.
3. A later `update` receives the response through `host_truapi_poll` and
   submits `truapi-roundtrip-ok` as save data.

The checked `.polkavm` fixture is built with the repository's pinned guest
build inputs and consumed unchanged by native and browser conformance tests.
