import { PolkadotClient } from "polkadot-api";
import { kreivo } from "@polkadot-api/descriptors";
import { WebAuthn as PasskeysAuthenticator } from "@virtonetwork/authenticators-webauthn";
import { KreivoPassSigner } from "@virtonetwork/signer";

export type UnsignedExtrinsic = any;

export interface BatchAllOptions {
  calls: UnsignedExtrinsic[];
}

export interface BatchOptions {
  calls: UnsignedExtrinsic[];
}

export interface UtilityResult {
  hash: string;
  blockHash?: string;
  success: boolean;
  error?: string;
}

export default class Utility {
  constructor(
    private readonly clientFactory: () => Promise<PolkadotClient>
  ) {}

  private async getClient(): Promise<PolkadotClient> {
    return this.clientFactory();
  }

  /**
   * Execute multiple transactions atomically using batch_all
   * All transactions will be reverted if any single transaction fails
   * 
   * @param sessionSigner - Session key signer (null to use main key)
   * @param options - Batch options with array of unsigned extrinsics
   * @returns Promise with the batch result
   */
  async batchAll(
    sessionSigner: any | null,
    options: BatchAllOptions,
  ): Promise<UtilityResult> {
    try {
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      
      console.log("Creating batch_all with", options.calls.length, "calls");
      
      // Extract decodedCall from each Transaction object
      const callsData = options.calls.map(call => {
        if (call && call.decodedCall) {
          console.log("Extracted call:", call.decodedCall);
          return call.decodedCall;
        }
        throw new Error("Invalid call format - missing decodedCall");
      });
      
      // Create batch_all transaction with the extracted call data
      const batchTx = kreivoApi.tx.Utility.batch_all({
        calls: callsData
      });

      let txRes = await batchTx.signAndSubmit(sessionSigner);

      console.log("Batch all successful:", txRes);
      return {
        hash: txRes.txHash,
        success: true,
      };
    } catch (error) {
      console.error("Batch all failed:", error);
      return {
        hash: "",
        success: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }

  /**
   * Execute multiple transactions with batch (non-atomic)
   * Some transactions may succeed even if others fail
   * 
   * @param sessionSigner - Session key signer (null to use main key)
   * @param options - Batch options with array of unsigned extrinsics
   * @returns Promise with the batch result
   */
  async batch(
    sessionSigner: any | null,
    options: BatchOptions,
  ): Promise<UtilityResult> {
    try {
      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);

      console.log("Creating batch with", options.calls.length, "calls");
      
      // Extract decodedCall from each Transaction object
      const callsData = options.calls.map(call => {
        if (call && call.decodedCall) {
          return call.decodedCall;
        }
        throw new Error("Invalid call format - missing decodedCall");
      });

      const batchTx = kreivoApi.tx.Utility.batch({
        calls: callsData
      });

      let txRes = await batchTx.signAndSubmit(sessionSigner);

      console.log("Batch successful:", txRes);
      return {
        hash: txRes.txHash,
        success: true,
      };
    } catch (error) {
      console.error("Batch failed:", error);
      return {
        hash: "",
        success: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }
} 