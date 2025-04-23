export type BaseProfile = {
    id: string;
    name?: string;
};

export type User<Profile> = {
    profile: Profile;
};

export type Command = {
    url: string;
    body: any;
    hex: string;
};

export enum WalletType {
    VIRTO = "virto",
    POLKADOT = "polkadot"
}
