# @virtonetwork/libwallet

This library enables users to work with [`libwallet`][1] on Javascript, using WASM.

## Current status

This library is a WORK IN PROGRESS, and so far, it's enabled to be used on Node environments.

## Usage

```javascript
import { Wallet } from '@virtonetwork/libwallet';

const wallet = new Wallet();

console.log(wallet.phrase); // -> "myself web subject call unfair return skull fatal radio spray insect fall twist ladder audit jump gravity modify search only blouse review receive south"
console.log([...wallet.address]); // -> [ 108, 204, 206, 223, 179, 1, 220, 225, 205, 117, 149, 151, 188, 225, 113, 10, 136, 122, 112, 31, 72, 132, 118, 58, 116, 31, 226, 197, 27, 238, 54, 17 ]
console.log(wallet.address.toHex()); // -> "0x6ccccedfb301dce1cd759597bce1710a887a701f4884763a741fe2c51bee3611"

const sig = wallet.sign(Buffer.from("my message"));
console.log(wallet.verify(Buffer.from("my message"), sig)); // -> true
```

[1]: https://github.com/virto-network/libwallet