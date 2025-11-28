import { PolkadotSigner } from "polkadot-api";
import { SignFn } from "../types";
import { getPolkadotSigner } from "polkadot-api/signer";
import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import { Blake2256 } from '@polkadot-api/substrate-bindings';
import { mergeUint8 } from "polkadot-api/utils";

export interface SerializableSignerData {
  publicKey: string;
  miniSecret: string;
  derivationPath: string;
  hashedUserId?: string;
  address?: string;
}

/**
 * Utility to serialize and deserialize signers
 * 
 * Allows converting a PolkadotSigner 
 * into JSON data that can be stored in any storage, and then recreate it.
 */
export class SignerSerializer {
  /**
   * Converts a signer into serializable data
   */
  static serialize(
    recreationData: {
      miniSecret: Uint8Array;
      derivationPath: string;
      originalPublicKey: Uint8Array;
      hashedUserId?: Uint8Array;
      address?: string;
    }
  ): SerializableSignerData {
    return {
      publicKey: Buffer.from(recreationData.originalPublicKey).toString('hex'),
      miniSecret: Buffer.from(recreationData.miniSecret).toString('hex'),
      derivationPath: recreationData.derivationPath,
      hashedUserId: recreationData.hashedUserId ? Buffer.from(recreationData.hashedUserId).toString('hex') : undefined,
      address: recreationData.address
    };
  }

  /**
   * Recreates a signer from serializable data
   */
  static deserialize(data: SerializableSignerData): PolkadotSigner & { sign: SignFn } {
    const miniSecret = new Uint8Array(Buffer.from(data.miniSecret, 'hex'));
    const originalPublicKey = new Uint8Array(Buffer.from(data.publicKey, 'hex'));

    // Recreate the keypair
    const derive = sr25519CreateDerive(miniSecret);
    const keypair = derive(data.derivationPath);

    // Recreate the signer
    const signer = getPolkadotSigner(keypair.publicKey, "Sr25519", keypair.sign);

    // Add the sign function
    Object.defineProperty(signer, "sign", {
      value: keypair.sign,
      configurable: false,
    });

    // Restore the modified publicKey if it exists
    if (data.hashedUserId) {
      const hashedUserId = new Uint8Array(Buffer.from(data.hashedUserId, 'hex'));
      signer.publicKey = Blake2256(
        mergeUint8(new Uint8Array(32).fill(0), hashedUserId)
      );
    } else {
      signer.publicKey = originalPublicKey;
    }

    // Restore the server address if it exists
    if (data.address) {
      Object.defineProperty(signer, "_serverAddress", {
        value: data.address,
        writable: false,
        enumerable: false,
        configurable: false,
      });
    }

    return signer as PolkadotSigner & { sign: SignFn };
  }

  /**
   * Verifies if the data is from a serialized signer
   */
  static isSerializableSignerData(data: any): data is SerializableSignerData {
    return data &&
      typeof data === 'object' &&
      typeof data.publicKey === 'string' &&
      typeof data.miniSecret === 'string' &&
      typeof data.derivationPath === 'string';
  }
}
