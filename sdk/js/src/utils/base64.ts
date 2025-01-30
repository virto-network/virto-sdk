export function fromBase64(b64: string): ArrayBuffer {
    const bin = atob(b64);
    const arr = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) {
        arr[i] = bin.charCodeAt(i);
    }
    return arr.buffer;
}

export function toBase64(buffer: Uint8Array): string {
    let binary = "";
    for (let i = 0; i < buffer.length; i++) {
        binary += String.fromCharCode(buffer[i] as number);
    }
    return btoa(binary);
}
