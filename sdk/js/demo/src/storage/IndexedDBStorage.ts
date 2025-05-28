import { IStorage } from "../../../src/storage";

/**
 * Example IndexedDB implementation for client-side session storage
 * 
 * Usage:
 * ```typescript
 * import { SDK } from "@virto-sdk/js";
 * import { IndexedDBStorage } from "./storage/IndexedDBStorage";
 * 
 * const indexedDBStorage = new IndexedDBStorage('VirtoSessions', 'sessions');
 * 
 * const sdk = new SDK({
 *   storage: indexedDBStorage
 * });
 * ```
 */
export class IndexedDBStorage<T> implements IStorage<T> {
    private dbName: string;
    private storeName: string;
    private db: IDBDatabase | null = null;

    constructor(dbName: string = 'VirtoSessions', storeName: string = 'sessions') {
        this.dbName = dbName;
        this.storeName = storeName;
    }

    private async openDB(): Promise<IDBDatabase> {
        if (this.db) {
            return this.db;
        }

        return new Promise((resolve, reject) => {
            const request = indexedDB.open(this.dbName, 1);

            request.onerror = () => reject(request.error);
            request.onsuccess = () => {
                this.db = request.result;
                resolve(request.result);
            };

            request.onupgradeneeded = (event) => {
                const db = (event.target as IDBOpenDBRequest).result;
                if (!db.objectStoreNames.contains(this.storeName)) {
                    db.createObjectStore(this.storeName, { keyPath: 'id' });
                }
            };
        });
    }

    private async getTransaction(mode: IDBTransactionMode = 'readonly'): Promise<IDBObjectStore> {
        const db = await this.openDB();
        const transaction = db.transaction([this.storeName], mode);
        return transaction.objectStore(this.storeName);
    }

    async store(key: string, session: T): Promise<void> {
        const store = await this.getTransaction('readwrite');
        
        return new Promise((resolve, reject) => {
            const request = store.put({ id: key, data: session });
            request.onerror = () => reject(request.error);
            request.onsuccess = () => resolve();
        });
    }

    async get(key: string): Promise<T | null> {
        const store = await this.getTransaction('readonly');
        
        return new Promise((resolve, reject) => {
            const request = store.get(key);
            request.onerror = () => reject(request.error);
            request.onsuccess = () => {
                const result = request.result;
                resolve(result ? result.data : null);
            };
        });
    }

    async getAll(): Promise<T[]> {
        const store = await this.getTransaction('readonly');
        
        return new Promise((resolve, reject) => {
            const request = store.getAll();
            request.onerror = () => reject(request.error);
            request.onsuccess = () => {
                const results = request.result;
                resolve(results.map((item: any) => item.data));
            };
        });
    }

    async remove(key: string): Promise<boolean> {
        const store = await this.getTransaction('readwrite');
        
        return new Promise((resolve, reject) => {
            const getRequest = store.get(key);
            getRequest.onerror = () => reject(getRequest.error);
            getRequest.onsuccess = () => {
                if (getRequest.result) {
                    const deleteRequest = store.delete(key);
                    deleteRequest.onerror = () => reject(deleteRequest.error);
                    deleteRequest.onsuccess = () => resolve(true);
                } else {
                    resolve(false);
                }
            };
        });
    }

    async clear(): Promise<void> {
        const store = await this.getTransaction('readwrite');
        
        return new Promise((resolve, reject) => {
            const request = store.clear();
            request.onerror = () => reject(request.error);
            request.onsuccess = () => resolve();
        });
    }

    close(): void {
        if (this.db) {
            this.db.close();
            this.db = null;
        }
    }
} 