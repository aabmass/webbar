import type { NotificationAction } from "./NotificationAction";
import type { NotificationUrgency } from "./NotificationUrgency";
export type NotificationItem = {
    id: number;
    app_name: string;
    icon: string;
    summary: string;
    body: string;
    urgency: NotificationUrgency;
    actions: Array<NotificationAction>;
};
