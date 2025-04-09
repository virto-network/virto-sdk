import Auth from "./auth";
import SessionManager from "./manager";
import { SimpleWalletFactory } from "./factory/simpleWalletFactory";
import { SubeFn, JsWalletBuilder } from "./wallet";
import { WalletType } from "./factory/walletFactory";

interface SDKOptions {
    federate_server: string;
    provider_url: string;
    config: {
        wallet: WalletType;
    };
}

export default class SDK {
    private _auth: Auth;

    constructor(
        options: SDKOptions,
        subeFn: SubeFn,
        jsWalletFn: JsWalletBuilder
    ) {
        const factory = new SimpleWalletFactory(subeFn, jsWalletFn, options.provider_url);
        const manager = new SessionManager(factory);
        this._auth = new Auth(options.federate_server, manager, options.config.wallet);
    }

    public get auth() {
        return this._auth;
    }
}
