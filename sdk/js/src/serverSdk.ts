import { getWsProvider } from "polkadot-api/ws-provider/node";
import ServerAuth from "./serverAuth";
import { ServerSDKOptions } from "./types";
import { createClient } from "polkadot-api";
import { InMemoryImpl, IStorage, SerializableSignerData } from "./storage";

/**
 * Server version of the SDK
 * This class only contains components that do NOT depend on browser APIs
 * and can be executed in a Node.js environment
 */
export default class ServerSDK {
    private _auth: ServerAuth;
    private _client: any = null;
    private _provider: any = null;

    /**
     * Creates a new ServerSDK instance
     * 
     * @param options - Configuration options for the server SDK
     * @param storage - Optional storage implementation for sessions
     */
    constructor(
        options: ServerSDKOptions,
        storage?: IStorage<SerializableSignerData>,
    ) {

        this._provider = getWsProvider(options.provider_url);
        this._client = createClient(this._provider);

        const getClient = async () => {
            return this._client;
        };


        if (!storage) {
            storage = new InMemoryImpl<SerializableSignerData>();
        }

        // Create ServerAuth with JWT configuration
        this._auth = new ServerAuth(
            options.federate_server,
            getClient,
            storage,
            {
                secret: options.config.jwt.secret,
                expiresIn: options.config.jwt.expiresIn
            }
        );
    }

    public get auth() {
        return this._auth;
    }
} 