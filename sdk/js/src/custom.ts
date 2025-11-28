import { Binary } from "polkadot-api";
import { kreivo } from "@virtonetwork/sdk/descriptors";
import TransactionQueue from "./transactionQueue";
import { TransactionResult } from "./types";

export interface CustomTxOptions {
  callDataHex: string; 
}

export default class CustomModule {
  constructor(
    private readonly clientFactory?: () => Promise<any>,
    private readonly transactionQueue?: TransactionQueue
  ) {}

  private async getClient(): Promise<any> {
    if (!this.clientFactory) {
      throw new Error("Client factory not configured");
    }
    return await this.clientFactory();
  }

  /**
   * Submit a custom transaction call (WAITS FOR INCLUSION)
   * Returns when transaction is included in block
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Custom transaction options including call data
   * @returns Promise with transaction result including hash
   * 
   * @example
   * const result = await custom.submitCallAsync(auth.sessionSigner, {
   *   callDataHex: "0x..."
   * });
   * console.log("Transaction included:", result.hash);
   */
  async submitCallAsync(sessionSigner: any | null, options: CustomTxOptions): Promise<TransactionResult> {
    if (!this.transactionQueue) {
      throw new Error("TransactionQueue not configured");
    }

    try {
      if (!sessionSigner) {
        throw new Error("Session signer is required for submitCallAsync");
      }

      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      const transaction = await kreivoApi.txFromCallData(Binary.fromHex(options.callDataHex));
      
      const transactionId = await this.transactionQueue.addTransaction(
        transaction,
        sessionSigner
      );

      const result = await this.transactionQueue.executeTransaction(transactionId, sessionSigner);

      return {
        ok: result.included,
        hash: result.hash,
        error: result.error
      };
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }
} 
