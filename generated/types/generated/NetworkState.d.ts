import type { Connectivity } from "./Connectivity";
import type { NetworkLink } from "./NetworkLink";
export type NetworkState = {
    available: boolean;
    connectivity: Connectivity;
    link: NetworkLink;
};
