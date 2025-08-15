import { TransactionConfirmationLevel } from './types';
import NonceManager from './nonceManager';
import TransactionQueue from './transactionQueue';

export interface TransactionExecutorOptions {
  getClient: () => Promise<any>;
  nonceManager: NonceManager;
  transactionQueue: TransactionQueue;
  confirmationLevel: TransactionConfirmationLevel;
}

export default class TransactionExecutor {
  private getClient: () => Promise<any>;
  private nonceManager: NonceManager;
  private transactionQueue: TransactionQueue;
  private confirmationLevel: TransactionConfirmationLevel;

  constructor(options: TransactionExecutorOptions) {
    this.getClient = options.getClient;
    this.nonceManager = options.nonceManager;
    this.transactionQueue = options.transactionQueue;
    this.confirmationLevel = options.confirmationLevel;
  }

  async executeTransaction(id: string, transaction: any, sessionSigner: any | null): Promise<{ included: boolean; hash?: string; error?: string }> {
    try {
      let nonce: number | undefined;
      const transactionMetadata = this.transactionQueue.getTransaction(id);
      if (transactionMetadata?.params?.nonce) {
        nonce = transactionMetadata.params.nonce;
      }

      if (this.confirmationLevel === 'submitted') {
        return this.handleSubmittedLevel(id, transaction, sessionSigner, nonce);
      }

      return this.handleIncludedOrFinalizedLevel(id, transaction, sessionSigner, nonce);

    } catch (error) {
      console.error(`Transaction execution failed:`, { 
        id, 
        error: error instanceof Error ? error.message : String(error)
      });
      this.transactionQueue.updateTransactionFailed(
        id,
        error instanceof Error ? error.message : String(error)
      );
      return { 
        included: false, 
        hash: undefined,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  private async handleSubmittedLevel(id: string, transaction: any, sessionSigner: any | null, nonce?: number): Promise<{ included: boolean; hash?: string; error?: string }> {
    const submission = await transaction.signAndSubmit(sessionSigner, nonce ? { nonce } : undefined);
    
    const blockHash = submission.block?.hash;
    this.transactionQueue.updateTransactionSubmitted(id, submission.txHash, blockHash);

    this.startBackgroundProcessing(id, submission);

    return { included: true, hash: submission.txHash };
  }

  private async handleIncludedOrFinalizedLevel(id: string, transaction: any, sessionSigner: any | null, nonce?: number): Promise<{ included: boolean; hash?: string; error?: string }> {
    const txSubmission = await transaction.signAndSubmit(sessionSigner, nonce ? { nonce } : undefined);

    const blockHash = txSubmission.block?.hash;
    this.transactionQueue.updateTransactionSubmitted(id, txSubmission.txHash, blockHash);

    if (this.confirmationLevel === 'included' || this.confirmationLevel === 'finalized') {
      const inclusionResult = await this.waitForInclusion(id, txSubmission);
      if (!inclusionResult.success) {
        return {
          included: false,
          hash: inclusionResult.hash,
          error: inclusionResult.error
        };
      }

      if (this.confirmationLevel === 'included') {
        this.waitForFinalization(id, txSubmission).catch(error => {
          console.error(`Background finalization failed:`, { id, error });
        });

        return {
          included: true,
          hash: txSubmission.txHash,
        };
      }
    }

    if (this.confirmationLevel === 'finalized') {
      const finalizedResult = await txSubmission.finalized;
      
      this.transactionQueue.updateTransactionFinalized(id, {
        hash: txSubmission.txHash,
        blockHash: finalizedResult?.block?.hash,
        success: true
      });

      return {
        included: true,
        hash: txSubmission.txHash,
      };
    }

    throw new Error(`Invalid confirmation level: ${this.confirmationLevel}`);
  }

  private async waitForInclusion(id: string, txSubmission: any): Promise<{ success: boolean; hash?: string; error?: string }> {
    const includedResult = await txSubmission.included;
    
    const blockHash = txSubmission.block?.hash || includedResult?.block?.hash;
    
    this.transactionQueue.updateTransactionIncluded(
      id,
      txSubmission.txHash,
      blockHash
    );

    const success = await txSubmission.ok;
    const error = success ? null : txSubmission.dispatchError || 'Transaction failed in block';

    if (!success) {
      console.error(`Transaction failed in block:`, { 
        id, 
        error 
      });
      this.transactionQueue.updateTransactionFailed(id, error);
      return { 
        success: false,
        hash: txSubmission.txHash,
        error: error
      };
    }

    return { success: true, hash: txSubmission.txHash };
  }

  private startBackgroundProcessing(id: string, submission: any): void {
    (async () => {
      try {
        const included = await submission.included;
        const blockHash = submission.block?.hash || included?.block?.hash;
        this.transactionQueue.updateTransactionIncluded(id, submission.txHash, blockHash);
      } catch (e) {
        console.error(`Inclusion wait failed (background):`, { id, error: e instanceof Error ? e.message : String(e) });
      }
      try {
        await this.waitForFinalization(id, submission);
      } catch (e) {
        console.error(`Finalization failed (background):`, { id, error: e instanceof Error ? e.message : String(e) });
      }
    })();
  }

  private async waitForFinalization(id: string, txSubmission: any): Promise<void> {
    try {
      await txSubmission.finalized;

      this.transactionQueue.updateTransactionFinalized(id, {
        hash: txSubmission.txHash,
        success: true
      });
    } catch (finalizationError) {
      console.error(`Transaction finalization failed (background):`, { 
        id, 
        error: finalizationError instanceof Error ? finalizationError.message : String(finalizationError)
      });
      this.transactionQueue.updateTransactionFinalized(id, {
        hash: txSubmission.txHash,
        blockHash: txSubmission.block?.hash,
        success: false,
        error: finalizationError instanceof Error ? finalizationError.message : String(finalizationError)
      });
    }
  }
} 