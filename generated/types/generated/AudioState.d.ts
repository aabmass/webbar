import type { AudioDevice } from "./AudioDevice";
export type AudioState = {
    available: boolean;
    outputs: Array<AudioDevice>;
    inputs: Array<AudioDevice>;
};
