import type { SwayWorkspace } from "./SwayWorkspace";
export type SwayState = {
    workspaces: Array<SwayWorkspace>;
    focused_title: string;
    mode: string;
};
