export function hexToUint8Array(hex: string): Uint8Array {
    if (hex.startsWith('0x')) {
        hex = hex.slice(2);
    }
    const length = hex.length / 2;
    const bytes = new Uint8Array(length);
    for (let i = 0; i < length; i++) {
        bytes[i] = parseInt(hex.substring(i * 2, i * 2 + 2), 16);
    }
    return bytes;
}

export function arrayBufferToBase64Url(buffer: any) {
    const bytes = new Uint8Array(buffer);
    let str = btoa(String.fromCharCode(...bytes));
    // Ajuste para que sea “URL-safe”
    return str.replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');
  }

export function toBase64(buffer: Uint8Array): string {
    let binary = "";
    for (let i = 0; i < buffer.length; i++) {
        binary += String.fromCharCode(buffer[i] as number);
    }
    return btoa(binary);
}
export function fromBase64Url(str: string): ArrayBuffer {
    console.log("fromBase64Url", str);
    let b64 = str.replace(/-/g, '+').replace(/_/g, '/') + "=";
    console.log("b64", b64);
    while (b64.length % 4) {
        b64 += '=';
    }
    return new Uint8Array(atob(b64).split('').map(c => c.charCodeAt(0)));
}