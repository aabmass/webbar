import { type WebbarApi } from "./preload";
declare global {
    interface Window {
        webbar: WebbarApi;
        __webbarOutput?: string;
        __webbarReceive?: (event: Record<string, unknown>) => void;
        webkit?: {
            messageHandlers?: {
                bridge?: {
                    postMessage: (msg: string) => void;
                };
            };
        };
    }
}
