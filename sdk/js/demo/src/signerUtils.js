import {
  entropyToMiniSecret,
  generateMnemonic,
  mnemonicToEntropy,
} from "@polkadot-labs/hdkd-helpers";
import { ed25519CreateDerive } from "@polkadot-labs/hdkd";
import { Binary } from "polkadot-api";

export function createEd25519Signer(mnemonic = null, derivationPath = '//default') {
  // Generate or use provided mnemonic
  const phrase = mnemonic || generateMnemonic(256);

  const miniSecret = entropyToMiniSecret(mnemonicToEntropy(phrase));
  const derive = ed25519CreateDerive(miniSecret);

  const keypair = derive(derivationPath);

  return {
    signer: {
      sign: (bytes) => {
        const signature = keypair.sign(bytes);
        return signature;
      },
      publicKey: keypair.publicKey,
      signingType: "Ed25519",
    },
    mnemonic: phrase,
    publicKey: keypair.publicKey,
  };
}

export function storeMnemonic(userId, mnemonic) {
  try {
    localStorage.setItem(`substrate_mnemonic_${userId}`, mnemonic);
    console.log(`Stored mnemonic for user: ${userId}`);
  } catch (error) {
    console.error('Failed to store mnemonic:', error);
  }
}

export function getMnemonic(userId) {
  try {
    return localStorage.getItem(`substrate_mnemonic_${userId}`);
  } catch (error) {
    console.error('Failed to retrieve mnemonic:', error);
    return null;
  }
}

export function removeMnemonic(userId) {
  try {
    localStorage.removeItem(`substrate_mnemonic_${userId}`);
    console.log(`Removed mnemonic for user: ${userId}`);
  } catch (error) {
    console.error('Failed to remove mnemonic:', error);
  }
}

export function hasMnemonic(userId) {
  return getMnemonic(userId) !== null;
}

