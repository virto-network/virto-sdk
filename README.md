# Sube

One letter shorter than [subxt](https://github.com/paritytech/substrate-subxt) and a lot smaller, doing less things by desing.

The main focus of this library is submitting signed extrinsics to your Substrate based chain consuming as little resources as possible to eventually run in the browser and embedded hardware. It will be your responsability to sign extrinsics with a different tool(i.e. [libwallet](https://github.com/valibre-org/libwallet)) before you feed the [SCALE](https://github.com/paritytech/parity-scale-codec) encoded data to _sube_.

It supports multiple backends with `http` being the first one and websockets, some embedded friendly backend or [`smoldot`](https://github.com/paritytech/smoldot) based lighnode likely following.  
As additionally planned optional fetaure users will be able to provide an unsigned extrinsic in a human readable format(e.g. JSON) that will be encoded to SCALE making use of the type information available in the chain's ([V13](https://github.com/paritytech/frame-metadata/blob/main/frame-metadata/src/v13.rs)) metadata.

## Cli

For convenience _Sube_ is also a stand-alone cli in case you want to create scripts that submit transactions. Something like this should be possible in the near future.

```sh
echo '{"my": "extrinsic"}' | sube encode | wallet sign | sube submit
```

