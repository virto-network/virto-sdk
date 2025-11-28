import { kreivo } from "@virtonetwork/sdk/descriptors";
import { PolkadotClient } from "polkadot-api";
import TransactionQueue from "./transactionQueue";
import { TransactionResult } from "./types";

export interface BatchOptions {
  calls: any[];
}

export default class Utility {
  constructor(
    private readonly clientFactory: () => Promise<PolkadotClient>,
    private readonly transactionQueue?: TransactionQueue
  ) {}

  private async getClient(): Promise<PolkadotClient> {
    return await this.clientFactory();
  }

  /**
   * Execute a batch of transactions (WAITS FOR INCLUSION)
   * Returns when transaction is included in block
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Batch options including array of calls
   * @returns Promise with transaction result including hash
   * 
   * @example
   * const result = await utility.batchAsync(auth.sessionSigner, {
   *   calls: [transferExtrinsic, remarkExtrinsic]
   * });
   * console.log("Transaction included:", result.hash);
   */
  async batchAsync(
    sessionSigner: any | null,
    options: BatchOptions,
  ): Promise<TransactionResult> {
    if (!this.transactionQueue) {
      throw new Error("TransactionQueue not configured");
    }

    try {
      if (!sessionSigner) {
        throw new Error("Session signer is required for batchAsync");
      }
      
      const callsData = options.calls.map((c: any) => (c && c.decodedCall) ? c.decodedCall : c);

      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      const transaction = kreivoApi.tx.Utility.batch({
        calls: callsData,
      });
      
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

  /**
   * Execute a batch of transactions atomically (WAITS FOR INCLUSION)
   * Returns when transaction is included in block
   * 
   * @param sessionSigner - Optional session signer for faster transactions
   * @param options - Batch options including array of calls
   * @returns Promise with transaction result including hash
   * 
   * @example
   * const result = await utility.batchAllAsync(auth.sessionSigner, {
   *   calls: [transferExtrinsic, remarkExtrinsic]
   * });
   * console.log("Transaction included:", result.hash);
   */
  async batchAllAsync(
    sessionSigner: any | null,
    options: BatchOptions,
  ): Promise<TransactionResult> {
    if (!this.transactionQueue) {
      throw new Error("TransactionQueue not configured");
    }

    try {
      if (!sessionSigner) {
        throw new Error("Session signer is required for batchAllAsync");
      }
      
      const callsData = options.calls.map((c: any) => (c && c.decodedCall) ? c.decodedCall : c);

      const client = await this.getClient();
      const kreivoApi = client.getTypedApi(kreivo);
      const transaction = kreivoApi.tx.Utility.batch_all({
        calls: callsData,
      });
      
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

  /**
   * Creates an unsigned batch_all extrinsic for batch operations
   * 
   * @param options - Batch options including array of calls
   * @returns Promise with the unsigned extrinsic
   * 
   * @example
   * const extrinsic = await utility.createBatchAllExtrinsic({
   *   calls: [transferExtrinsic, remarkExtrinsic]
   * });
   */
  async createBatchAllExtrinsic(options: BatchOptions): Promise<any> {
    const client = await this.getClient();
    const kreivoApi = client.getTypedApi(kreivo);

    const callsData = options.calls.map((c: any) => (c && c.decodedCall) ? c.decodedCall : c);

    const extrinsic = kreivoApi.tx.Utility.batch_all({
      calls: callsData,
    });

    return extrinsic;
  }
} 