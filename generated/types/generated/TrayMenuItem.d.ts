export type TrayMenuItem = {
    id: number;
    label: string;
    enabled: boolean;
    visible: boolean;
    item_type: string;
    toggle_type: string;
    toggle_state: number;
    children: Array<TrayMenuItem>;
};
