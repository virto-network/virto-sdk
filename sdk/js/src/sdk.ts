import Auth from "./auth";
import SessionManager from "./manager";
import { SimpleWalletFactory } from "./factory/simpleWalletFactory";
import { SubeFn, JsWalletBuilder } from "./wallet";
import { WalletType } from "./factory/walletFactory";
import { IStorage } from "./storage";
import { getWsProvider } from "polkadot-api/ws-provider/web";

import { createClient } from "polkadot-api";
import { VOSCredentialsHandler } from "./vocCredentialHandler";

interface SDKOptions {
    federate_server: string;
    provider_url: string;
    config: {
        wallet: WalletType;
    };
}

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

    constructor(
        options: SDKOptions,
        subeFn: SubeFn,
        jsWalletFn: JsWalletBuilder,
        storage?: IStorage<any>,
    ) {
        const factory = new SimpleWalletFactory(subeFn, jsWalletFn, options.provider_url);
        const manager = new SessionManager(factory, storage);
        console.log(options.federate_server);
        const credentialsHandler = new VOSCredentialsHandler(options.federate_server);
        
        // Crear una función que cree el cliente de forma lazy con el proveedor correcto
        const getClient = async () => {
            const provider = getWsProvider(options.provider_url);
            return createClient(provider);
        };
        
        this._auth = new Auth(options.federate_server, credentialsHandler, getClient, manager, options.config.wallet);
    }

    public get auth() {
        return this._auth;
    }
}
