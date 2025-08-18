import ServerAuth from "./serverAuth";
import ServerManager from "./serverManager";
import { SubeFn, JsWalletBuilder } from "./wallet";
import { SimpleWalletFactory } from "./factory/simpleWalletFactory";
import { WalletType } from "./types";
import { IStorage } from "./storage";
import { Session } from "./manager";

interface ServerSDKOptions {
    federate_server: string;
    provider_url: string;
    config: {
        wallet: WalletType;
        jwt: {
            secret: string;
            expiresIn?: string;
        }
    };
}

/**
 * Server version of the SDK
 * This class only contains components that do NOT depend on browser APIs
 * and can be executed in a Node.js environment
 */
export default class ServerSDK {
    private _auth: ServerAuth;

    /**
     * Creates a new ServerSDK instance
     * 
     * @param options - Configuration options for the server SDK
     * @param storage - Optional storage implementation for sessions
     * @param subeFn - The sube function (not used in server environments)
     * @param jsWalletFn - The JavaScript wallet builder function (not used in server environments)
     */
    constructor(
        options: ServerSDKOptions,
        subeFn: SubeFn,
        jsWalletFn: JsWalletBuilder,
        storage?: IStorage<Session>,
    ) {
        const factory = new SimpleWalletFactory(subeFn, jsWalletFn, options.provider_url);

        const manager = new ServerManager(factory, storage);

        const defaultWallet = options.config?.wallet || WalletType.POLKADOT;

        // Create ServerAuth with JWT configuration
        this._auth = new ServerAuth(
            options.federate_server,
            manager,
            defaultWallet,
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