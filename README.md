# Sube

A client library and CLI for Substrate chains, doing less by design than [subxt](https://github.com/paritytech/substrate-subxt) with a big focus on size and portability so it can run in constrainted environments like the browser.

Making use of the type information in a chain's metadata(`v14 >=`) and powered by our [Scales](https://github.com/virto-network/scales) library, Sube allows automatic conversion between the [SCALE](https://github.com/paritytech/parity-scale-codec) binary format used by the blockchain with a human-readable representation like JSON without having to hardcode type information for each network. 
When submitting extrinsics Sube only does that, it's your responsability to sign the payload with a different tool first(e.g. [libwallet](https://github.com/valibre-org/libwallet)) before you feed the extrinsic data to the library.

Sube supports multiple backends under different feature flags like `http`, `http-web` or `ws`/`wss`, with [future support](https://github.com/virto-network/sube/milestone/3) for a [`smoldot`](https://github.com/paritytech/smoldot) based light-node.  

## Cli

For convenience Sube is also a stand-alone cli. 

[![asciicast](https://asciinema.org/a/443014.svg)](https://asciinema.org/a/443014)

## Progressive decentralization

> 🛠️ ⚠️ [Upcoming feature](https://github.com/virto-network/sube/milestone/2)

The true _raison d'etre_ of Sube is not to create yet another Substrate client but to enable the Virto.Network and any project in the ecosystem to reach a broader audience of end-users and developers by lowering the technical entry barrier and drasticaly improving the overal user experience of interacting with blockchains. We call it **progressive decentralization**.

When paired with our plugin runtime [Valor](https://github.com/virto-network/valor), Sube can be exposed as an HTTP API that runs both in the server and the browser and be composed with other plugins to create higher level APIs that a client aplication can use from any plattform thanks to the ubiquitousness of HTTP.
We imagine existing centralized projects easily integrating with Substrate blockchains in the server with the option to progressively migrate to a decentralized set-up with whole backends later running in the user device(web browser included).  
But progressive decentralization goes beyond the migration of a centralized project, it's rather about giving users the the best experience by possibly combining the best of both worlds. A Sube powered application can start being served from a server to have an immediate response and 0 start-up time and since plugins can be hot-swapped, the blockchain backend can be switched from HTTP to lightnode transparently without the application code ever realizing, giving our users with bad connectivity and slower devices the opportunity to enjoy the best possible user experience without compromizing decentralization.
