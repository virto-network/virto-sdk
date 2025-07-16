import Auth from "./auth";
import Transfer, { DefaultUserService } from "./transfer";
import System from "./system";
import Utility from "./utility";
import { IStorage } from "./storage";
import { getWsProvider } from "polkadot-api/ws-provider/web";

import { createClient } from "polkadot-api";
import { VOSCredentialsHandler } from "./vocCredentialHandler";

interface SDKOptions {
    federate_server: string;
    provider_url: string;
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
    private _transfer: Transfer;
    private _system: System;
    private _utility: Utility;

    constructor(
        options: SDKOptions,
        storage?: IStorage<any>,
    ) {
        console.log(options.federate_server);
        
        const credentialsHandler = new VOSCredentialsHandler(options.federate_server);
        
        // Crear una función que cree el cliente de forma lazy con el proveedor correcto
        const getClient = async () => {
            const provider = getWsProvider(options.provider_url);
            return createClient(provider);
        };

        const userService = new DefaultUserService(options.federate_server);
        
        this._auth = new Auth(options.federate_server, credentialsHandler, getClient);
        this._transfer = new Transfer(getClient, userService);
        this._system = new System(getClient);
        this._utility = new Utility(getClient);
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
}
