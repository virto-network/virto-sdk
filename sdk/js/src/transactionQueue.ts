export type TransactionStatus = 'pending' | 'included' | 'finalized' | 'failed';

export interface TransactionMetadata {
  id: string;
  status: TransactionStatus;
  hash?: string;
  blockHash?: string;
  error?: string;
  timestamp: number;
  params: any;
  result?: any;
}

export type TransactionEventType = 'submitted' | 'included' | 'finalized' | 'failed';

export interface TransactionEvent {
  id: string;
  type: TransactionEventType;
  transaction: TransactionMetadata;
  timestamp: number;
}

export type TransactionEventCallback = (event: TransactionEvent) => void;

import { TransactionConfirmationLevel } from './types';
import NonceManager from './nonceManager';
import TransactionExecutor from './transactionExecutor';

export default class TransactionQueue {
  private transactions: Map<string, TransactionMetadata> = new Map();
  private eventListeners: Set<TransactionEventCallback> = new Set();
  private executor?: TransactionExecutor;
  private nonceManager?: NonceManager;
  private confirmationLevel: TransactionConfirmationLevel = 'included';
  private getAddressFromAuthenticator?: (sessionSigner: any) => string;
  
  constructor() {}

  setConfirmationLevel(level: TransactionConfirmationLevel): void {
    this.confirmationLevel = level;
  }

  setNonceManager(nonceManager: NonceManager): void {
    this.nonceManager = nonceManager;
  }

  setExecutor(executor: TransactionExecutor): void {
    this.executor = executor;
  }

  setAddressHelper(getAddressFromAuthenticator: (sessionSigner: any) => string): void {
    this.getAddressFromAuthenticator = getAddressFromAuthenticator;
  }

  async addTransaction(
    transaction: any,
    sessionSigner: any | null
  ): Promise<string> {
    let nonce: number;
    if (this.nonceManager && sessionSigner && this.getAddressFromAuthenticator) {
      try {
        const address = this.getAddressFromAuthenticator(sessionSigner);
        nonce = await this.nonceManager.getAndIncrementNonce(address);
      } catch (error) {
        console.error(`Error getting nonce for transaction:`, error);
        nonce = Date.now();
      }
    } else {
      nonce = Date.now();
    }

    const id = nonce.toString();
    const transactionMetadata: TransactionMetadata = {
      id,
      status: 'pending',
      timestamp: Date.now(),
      params: { 
        _preBuiltTransaction: transaction,
        sessionSigner: sessionSigner,
        nonce: nonce
      }
    };

    this.transactions.set(id, transactionMetadata);
    this.emitEvent('submitted', transactionMetadata);

    return id;
  }

  async executeTransaction(id: string, sessionSigner: any | null): Promise<{ included: boolean; hash?: string; error?: string }> {
    const transaction = this.transactions.get(id);
    if (!transaction) {
      throw new Error(`Transaction ${id} not found in queue`);
    }

    if (!this.executor) {
      throw new Error('Transaction executor not configured');
    }

    const preBuiltTransaction = transaction.params._preBuiltTransaction;
    if (!preBuiltTransaction) {
      throw new Error(`Pre-built transaction not found for transaction ${id}`);
    }

    if (this.confirmationLevel === 'submitted') {
      (async () => {
        try {
          await this.executor!.executeTransaction(id, preBuiltTransaction, sessionSigner);
        } catch (error) {
          const errorMessage = error instanceof Error ? error.message : String(error);
          this.updateTransactionFailed(id, errorMessage);
        }
      })();

      return { included: true, hash: undefined };
    }

    try {
      const result = await this.executor.executeTransaction(id, preBuiltTransaction, sessionSigner);
      return result;
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      this.updateTransactionFailed(id, errorMessage);
      throw error;
    }
  }

  updateTransactionSubmitted(id: string, hash: string, blockHash?: string): void {
    const transaction = this.transactions.get(id);
    if (!transaction) return;

    transaction.status = 'pending';
    transaction.hash = hash;
    transaction.blockHash = blockHash;
    
    this.transactions.set(id, transaction);
    this.emitEvent('submitted', transaction);
  }

  updateTransactionIncluded(id: string, hash: string, blockHash?: string): void {
    const transaction = this.transactions.get(id);
    if (!transaction) return;

    transaction.status = 'included';
    transaction.hash = hash;
    transaction.blockHash = blockHash;
    
    this.transactions.set(id, transaction);
    this.emitEvent('included', transaction);
  }

  updateTransactionFinalized(id: string, result?: any): void {
    const transaction = this.transactions.get(id);
    if (!transaction) return;

    transaction.status = 'finalized';
    transaction.result = result;
    
    this.transactions.set(id, transaction);
    this.emitEvent('finalized', transaction);
  }

  updateTransactionFailed(id: string, error: string): void {
    const transaction = this.transactions.get(id);
    if (!transaction) return;

    transaction.status = 'failed';
    transaction.error = error;
    
    this.transactions.set(id, transaction);
    this.emitEvent('failed', transaction);
  }

  getTransaction(id: string): TransactionMetadata | undefined {
    return this.transactions.get(id);
  }

  getAllTransactions(): TransactionMetadata[] {
    return Array.from(this.transactions.values()).sort((a, b) => b.timestamp - a.timestamp);
  }

  getTransactionsByStatus(status: TransactionStatus): TransactionMetadata[] {
    return this.getAllTransactions().filter(tx => tx.status === status);
  }

  clearCompletedTransactions(): void {
    for (const [id, transaction] of this.transactions.entries()) {
      if (transaction.status === 'finalized' || transaction.status === 'failed') {
        this.transactions.delete(id);
      }
    }
  }

  addEventListener(callback: TransactionEventCallback): void {
    this.eventListeners.add(callback);
  }

  removeEventListener(callback: TransactionEventCallback): void {
    this.eventListeners.delete(callback);
  }

  removeAllEventListeners(): void {
    this.eventListeners.clear();
  }

  private emitEvent(type: TransactionEventType, transaction: TransactionMetadata): void {
    const event: TransactionEvent = {
      id: transaction.id,
      type,
      transaction: { ...transaction },
      timestamp: Date.now()
    };
    
    this.eventListeners.forEach(callback => {
      try {
        callback(event);
      } catch (error) {
        console.error('Error in transaction event callback:', error);
      }
    });
  }
} 