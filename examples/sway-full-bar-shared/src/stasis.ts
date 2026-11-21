export type StasisWatchState = {
  state: string;
  paused: boolean;
  manually_paused: boolean;
  profile: string;
};

export type BlameCategory = {
  active: boolean;
  count: number;
  sources: string[];
};

export type DbusHold = {
  sender: string;
  application: string | null;
  process: string | null;
  reason: string | null;
};

export type Login1IdleHold = {
  who: string;
  why: string;
  process: string | null;
};

export type BlameSnapshot = {
  progression_blocked: boolean;
  manual_pause: boolean;
  system_pause: boolean;
  browser_source_capture: boolean;
  app_inhibitors: BlameCategory;
  media_inhibitors: BlameCategory;
  game_inhibitors: BlameCategory;
  suspend_app_inhibitors: BlameCategory;
  suspend_media_inhibitors: BlameCategory;
  dbus_holds: DbusHold[];
  login1_idle_holds: Login1IdleHold[];
};

export type BlockerItem = {
  key: string;
  name: string;
  detail?: string;
};

export const DEFAULT_PROFILE = "default";

export function parseProfilesOutput(stdout: string): string[] {
  const seen = new Set<string>([DEFAULT_PROFILE]);
  const profiles: string[] = [DEFAULT_PROFILE];

  for (const rawLine of stdout.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (
      !line ||
      line === "Profiles:" ||
      line.toLowerCase().startsWith("no profiles")
    ) {
      continue;
    }
    if (!seen.has(line)) {
      seen.add(line);
      profiles.push(line);
    }
  }

  return profiles;
}

export function formatProfileLabel(profile: string): string {
  return profile.replace(/_/g, "-");
}

export function extractBlockers(snapshot: BlameSnapshot | null): BlockerItem[] {
  if (!snapshot) return [];
  const items: BlockerItem[] = [];

  for (const hold of snapshot.dbus_holds) {
    const name =
      hold.application?.trim() || hold.process?.trim() || hold.sender;
    items.push({
      key: `dbus:${hold.sender}:${hold.application ?? ""}:${hold.reason ?? ""}`,
      name,
      detail: hold.reason?.trim() || "D-Bus inhibit",
    });
  }

  for (const hold of snapshot.login1_idle_holds) {
    const name = hold.who.trim() || hold.process?.trim() || "systemd-inhibit";
    items.push({
      key: `login1:${name}:${hold.why}`,
      name,
      detail: hold.why.trim() || "Suspend hold",
    });
  }

  const pushCategory = (
    prefix: string,
    label: string,
    cat: BlameCategory,
  ) => {
    if (!cat.active || cat.count === 0) return;
    if (cat.sources.length > 0) {
      for (const src of cat.sources) {
        items.push({
          key: `${prefix}:${src}`,
          name: src,
          detail: label,
        });
      }
    } else {
      items.push({
        key: `${prefix}:count`,
        name: label,
        detail: `${cat.count} active`,
      });
    }
  };

  pushCategory("app", "App", snapshot.app_inhibitors);
  pushCategory("media", "Media", snapshot.media_inhibitors);
  pushCategory("game", "Game", snapshot.game_inhibitors);
  pushCategory("sapp", "Suspend hold", snapshot.suspend_app_inhibitors);
  pushCategory("smedia", "Media (suspend)", snapshot.suspend_media_inhibitors);

  if (snapshot.browser_source_capture) {
    items.push({
      key: "browser-capture",
      name: "Browser",
      detail: "Audio/screen capture",
    });
  }

  if (snapshot.system_pause) {
    items.push({
      key: "system-pause",
      name: "System",
      detail: "Sleep or lid hold",
    });
  }

  return items;
}
