import Auth from "./auth";
import Transfer from "./transfer";
import System from "./system";
import Utility from "./utility";
import CustomModule from "./custom";
import TransactionQueue, { TransactionEventCallback } from "./transactionQueue";
import NonceManager from "./nonceManager";
import TransactionExecutor from "./transactionExecutor";
import { IStorage } from "./storage";
import { getWsProvider } from "polkadot-api/ws-provider/web";
import { DefaultUserService } from "./services/userService";

import { createClient } from "polkadot-api";
import { VOSCredentialsHandler } from "./vocCredentialHandler";

import { SDKOptions, TransactionConfirmationLevel } from './types';
import Membership from "./membership";

// Función para obtener el proveedor WebSocket correcto según el entorno
// async function getWebSocketProvider(url: string) {
//     // if (typeof window !== "undefined") {
//         // Entorno del navegador
//         const { getWsProvider } = await import("polkadot-api/ws-provider/web");
//         return getWsProvider(url);
//     // } else {
//     //     // Entorno Node.js
//     //     const { getWsProvider } = await import("polkadot-api/ws-provider/node");
//     //     return getWsProvider(url);
//     // }
// }

export default class SDK {
    private _auth: Auth;
    private _transfer: Transfer;
    private _system: System;
    private _utility: Utility;
    private _custom: CustomModule;
    private _transactionQueue: TransactionQueue;
    private _nonceManager: NonceManager;
    private _memberships: Membership;
    private _confirmationLevel: TransactionConfirmationLevel;
    private _client: any = null;
    private _provider: any = null;

    constructor(
        options: SDKOptions,
        storage?: IStorage<any>,
    ) {
        console.log(options.federate_server);
        
        this._confirmationLevel = options.confirmation_level || 'included';
        
        this._transactionQueue = new TransactionQueue();
        const credentialsHandler = new VOSCredentialsHandler(options.federate_server);
        
        // Create provider with status monitoring using the official WS provider
        const onStatusChanged = (status: any) => {
            if (options.onProviderStatusChange) {
                options.onProviderStatusChange(status);
            }
        };
        
        this._provider = getWsProvider(options.provider_url, onStatusChanged);
        this._client = createClient(this._provider);
        
        const getClient = async () => {
            return this._client;
        };

        const userService = new DefaultUserService(options.federate_server);
        
        this._nonceManager = new NonceManager(getClient);
        
        this._transactionQueue.setNonceManager(this._nonceManager);
        this._transactionQueue.setConfirmationLevel(this._confirmationLevel);
        
        this._auth = new Auth(options.federate_server, credentialsHandler, getClient, this._nonceManager);
        this._memberships = new Membership(options.federate_server);
        
        // Configure the address helper in transaction queue
        this._transactionQueue.setAddressHelper((sessionSigner) => {
            return this._auth.getAddressFromAuthenticator(sessionSigner);
        });
        
        this._transactionQueue.setAuthModule(this._auth);
        
        this._transfer = new Transfer(getClient, userService, this._transactionQueue);
        this._system = new System(getClient, this._transactionQueue);
        this._utility = new Utility(getClient, this._transactionQueue);
        this._custom = new CustomModule(getClient, this._transactionQueue);

        this.setupTransactionExecutor(getClient);
    }

    private setupTransactionExecutor(getClient: () => Promise<any>): void {
        const executor = new TransactionExecutor({
            getClient,
            nonceManager: this._nonceManager,
            transactionQueue: this._transactionQueue,
            confirmationLevel: this._confirmationLevel
        });

        this._transactionQueue.setExecutor(executor);
    }

    public get auth() {
        return this._auth;
    }

    public get transfer() {
        return this._transfer;
    }

    public get system() {
        return this._system;
    }

    public get utility() {
        return this._utility;
    }

    public get custom() {
        return this._custom;
    }

    public get nonceManager() {
        return this._nonceManager;
    }

    public get transactions() {
        return this._transactionQueue;
    }

    /**
     * Add event listener for transaction updates
     * 
     * @param callback - Function to call when transaction events occur
     * 
     * @example
     * sdk.onTransactionUpdate((event) => {
     *   console.log(`Transaction ${event.id} is now ${event.type}`);
     *   if (event.type === 'included') {
     *     console.log(`Hash: ${event.transaction.hash}`);
     *   }
     * });
     */
    public onTransactionUpdate(callback: TransactionEventCallback): void {
        this._transactionQueue.addEventListener(callback);
    }

    public removeTransactionListener(callback: TransactionEventCallback): void {
        this._transactionQueue.removeEventListener(callback);
    }

    public getTransactionHistory() {
        return this._transactionQueue.getAllTransactions();
    }

    public getPendingTransactions() {
        return this._transactionQueue.getTransactionsByStatus('pending');
    }

    public clearTransactionHistory(): void {
        this._transactionQueue.clearCompletedTransactions();
    }
}
