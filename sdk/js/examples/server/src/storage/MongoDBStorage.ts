import { IStorage } from "../../../../src/storage/index";
import { MongoClient, Db, Collection } from 'mongodb';

/**
 * MongoDB implementation for server-side session storage
 * 
 * Usage:
 * ```typescript
 * import { ServerSDK } from "@virto-sdk/js";
 * import { MongoDBStorage } from "./storage/MongoDBStorage";
 * 
 * const mongoStorage = new MongoDBStorage({
 *   url: 'mongodb://localhost:27017',
 *   dbName: 'virto_sessions'
 * });
 * 
 * const serverSDK = new ServerSDK({
 *   // ... other options
 * }, mongoStorage);
 * ```
 */
export class MongoDBStorage<T> implements IStorage<T> {
    private client: MongoClient;
    private db: Db | null = null;
    private collection: Collection | null = null;
    private dbName: string;
    private collectionName: string;

    constructor(options: {
        url?: string;
        dbName?: string;
        collectionName?: string;
        clientOptions?: any;
    } = {}) {
        const url = options.url || 'mongodb://localhost:27017';
        this.dbName = options.dbName || 'virto_sessions';
        this.collectionName = options.collectionName || 'sessions';
        
        this.client = new MongoClient(url, {
            maxPoolSize: 10,
            serverSelectionTimeoutMS: 5000,
            socketTimeoutMS: 45000,
            ...options.clientOptions
        });

        this.client.on('error', (err: any) => console.error('MongoDB Client Error', err));
    }

    async connect(): Promise<void> {
        try {
            await this.client.connect();
            this.db = this.client.db(this.dbName);
            this.collection = this.db.collection(this.collectionName);
            
            // Create index on key for better performance
            await this.collection.createIndex({ key: 1 }, { unique: true });
        } catch (error) {
            // Connection will be retried automatically
        }
    }

    async disconnect(): Promise<void> {
        try {
            await this.client.close();
        } catch (error) {
            console.error('Error disconnecting from MongoDB:', error);
        }
    }

    async store(key: string, session: T): Promise<void> {
        await this.connect();
        if (!this.collection) {
            throw new Error('MongoDB collection not initialized');
        }

        await this.collection.replaceOne(
            { key },
            { 
                key, 
                data: session,
                createdAt: new Date(),
                updatedAt: new Date()
            },
            { upsert: true }
        );
    }

    async get(key: string): Promise<T | null> {
        await this.connect();
        if (!this.collection) {
            throw new Error('MongoDB collection not initialized');
        }

        const document = await this.collection.findOne({ key });
        return document ? document.data : null;
    }

    async getAll(): Promise<T[]> {
        await this.connect();
        if (!this.collection) {
            throw new Error('MongoDB collection not initialized');
        }

        const documents = await this.collection.find({}).toArray();
        return documents.map((doc: any) => doc.data);
    }

    async remove(key: string): Promise<boolean> {
        await this.connect();
        if (!this.collection) {
            throw new Error('MongoDB collection not initialized');
        }

        const result = await this.collection.deleteOne({ key });
        return result.deletedCount > 0;
    }

    async clear(): Promise<void> {
        await this.connect();
        if (!this.collection) {
            throw new Error('MongoDB collection not initialized');
        }

        await this.collection.deleteMany({});
    }
} 