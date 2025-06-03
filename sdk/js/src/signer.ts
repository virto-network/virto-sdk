import {
    DEV_PHRASE,
    entropyToMiniSecret,
    mnemonicToEntropy,
    ss58Encode,
} from "@polkadot-labs/hdkd-helpers";

import { getPolkadotSigner } from "polkadot-api/signer";
import { sr25519CreateDerive } from "@polkadot-labs/hdkd";

const entropy = mnemonicToEntropy(DEV_PHRASE);
const seed = entropyToMiniSecret(entropy);
const derive = sr25519CreateDerive(seed);

// Example usage for generating a sr25519 keypair with hard derivation
const keyPair = derive("//Alice");
const publicKey = keyPair.publicKey;

export const polkadotSigner = getPolkadotSigner(
    publicKey,
    "Sr25519",
    keyPair.sign
);
export const publicAddress = ss58Encode(polkadotSigner.publicKey);
