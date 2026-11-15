import type { BluetoothAdapter } from "./BluetoothAdapter";
import type { BluetoothDevice } from "./BluetoothDevice";
export type BluetoothState = {
    available: boolean;
    adapters: Array<BluetoothAdapter>;
    devices: Array<BluetoothDevice>;
};
