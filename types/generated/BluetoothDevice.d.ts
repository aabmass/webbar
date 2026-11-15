export type BluetoothDevice = {
    path: string;
    name: string;
    address: string;
    paired: boolean;
    connected: boolean;
    battery: number | null;
};
