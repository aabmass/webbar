export type NetworkLink = {
    "kind": "wifi";
    name: string;
    ssid: string;
    strength: number;
} | {
    "kind": "wired";
    name: string;
} | {
    "kind": "other";
    name: string;
} | {
    "kind": "none";
};
