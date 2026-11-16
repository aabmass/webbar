import { useCallback, useEffect, useRef, useState } from "react";
import { IdleIcon } from "./Icons";
import { Popover } from "./Popover";

type StasisWatchState = {
  state: string;
  paused: boolean;
  manually_paused: boolean;
  profile: string;
};

const DEFAULT_PROFILE = "default";

function parseProfilesOutput(stdout: string): string[] {
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

function formatProfileLabel(profile: string): string {
  return profile.replace(/_/g, "-");
}

export function Stasis() {
  const [status, setStatus] = useState<StasisWatchState | null>(null);
  const [profiles, setProfiles] = useState<string[]>([DEFAULT_PROFILE]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const refreshProfiles = useCallback(async () => {
    try {
      const res = await window.webbar.exec(["stasis", "list", "profiles"]);
      if (res.code === 0) {
        setProfiles(parseProfilesOutput(res.stdout));
      }
    } catch {
      // Keep existing profiles list if stasis is temporarily unavailable
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let reconnectTimer: number | null = null;
    let controller = new AbortController();

    const startWatch = async () => {
      if (disposed) return;
      controller = new AbortController();

      try {
        await window.webbar.execStream(["stasis", "watch"], {
          signal: controller.signal,
          onStdout: (line) => {
            const trimmed = line.trim();
            if (!trimmed) return;
            try {
              const next = JSON.parse(trimmed) as Partial<StasisWatchState>;
              if (typeof next.state === "string") {
                setStatus({
                  state: next.state,
                  paused: Boolean(next.paused),
                  manually_paused: Boolean(next.manually_paused),
                  profile:
                    typeof next.profile === "string" && next.profile
                      ? next.profile
                      : DEFAULT_PROFILE,
                });
                setError(null);
              }
            } catch {
              // Ignore non-JSON lines
            }
          },
          onStderr: (line) => {
            const trimmed = line.trim();
            if (trimmed) {
              setError(trimmed);
            }
          },
          onExit: () => {
            if (disposed) return;
            reconnectTimer = window.setTimeout(() => {
              void startWatch();
            }, 2000);
          },
        });
      } catch (err) {
        if (disposed) return;
        if (err instanceof DOMException && err.name === "AbortError") {
          return;
        }
        setError(err instanceof Error ? err.message : String(err));
        reconnectTimer = window.setTimeout(() => {
          void startWatch();
        }, 5000);
      }
    };

    void startWatch();
    void refreshProfiles();

    return () => {
      disposed = true;
      if (reconnectTimer !== null) {
        window.clearTimeout(reconnectTimer);
      }
      controller.abort();
    };
  }, [refreshProfiles]);

  useEffect(() => {
    if (open) {
      void refreshProfiles();
    }
  }, [open, refreshProfiles]);

  const runStasisCommand = useCallback(async (argv: string[]) => {
    setBusy(true);
    setError(null);
    try {
      const res = await window.webbar.exec(argv);
      if (res.code !== 0) {
        const msg =
          res.stderr.trim() ||
          res.stdout.trim() ||
          `Command exited with ${res.code ?? "error"}`;
        setError(msg);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  const activeProfile = status?.profile ?? DEFAULT_PROFILE;
  const isPaused = Boolean(status?.paused || status?.manually_paused);
  const stateLabel = status
    ? isPaused
      ? status.manually_paused
        ? "inhibited"
        : "paused"
      : status.state
    : "offline";

  const pillClass = [
    "bar-btn",
    "stasis-btn",
    !status ? "is-dimmed" : "",
    isPaused ? "is-inhibited" : "",
    open ? "is-open" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={pillClass}
        title={
          error
            ? `Stasis: ${error}`
            : status
              ? `Stasis: ${formatProfileLabel(activeProfile)} (${stateLabel}) — click for controls, right-click to toggle inhibit`
              : "Stasis offline"
        }
        onClick={() => setOpen((prev) => !prev)}
        onContextMenu={(e) => {
          e.preventDefault();
          void runStasisCommand(["stasis", "toggle-inhibit"]);
        }}
      >
        <IdleIcon inhibited={isPaused} />
        <span className="stasis-label">
          {status ? formatProfileLabel(activeProfile) : "idle"}
        </span>
      </button>

      <Popover open={open} onClose={close} anchorRef={btnRef} align="end">
        <div className="stasis-popover">
          <div className="stasis-header">
            <span className="stasis-title">Stasis Idle</span>
            <span
              className={`stasis-badge${isPaused ? " is-inhibited" : ""}${!status ? " is-offline" : ""}`}
            >
              {stateLabel}
            </span>
          </div>

          {error && <div className="stasis-error">{error}</div>}

          <div className="stasis-actions">
            <button
              type="button"
              className={`bar-btn stasis-action-btn${status?.paused ? " is-active" : ""}`}
              disabled={busy || !status}
              onClick={() =>
                void runStasisCommand([
                  "stasis",
                  status?.paused ? "resume" : "pause",
                ])
              }
            >
              {status?.paused ? "Resume" : "Pause"}
            </button>
            <button
              type="button"
              className={`bar-btn stasis-action-btn${status?.manually_paused ? " is-active" : ""}`}
              disabled={busy || !status}
              onClick={() =>
                void runStasisCommand(["stasis", "toggle-inhibit"])
              }
            >
              {status?.manually_paused ? "Uninhibit" : "Inhibit"}
            </button>
          </div>

          <div className="stasis-section-label">Profiles</div>
          <div className="stasis-profile-list">
            {profiles.map((profile) => {
              const isCurrent = activeProfile === profile;
              return (
                <button
                  key={profile}
                  type="button"
                  className={`bar-btn stasis-profile-btn${isCurrent ? " is-current" : ""}`}
                  disabled={busy || !status}
                  onClick={() =>
                    void runStasisCommand([
                      "stasis",
                      "profile",
                      profile === DEFAULT_PROFILE ? "none" : profile,
                    ])
                  }
                >
                  <span>{formatProfileLabel(profile)}</span>
                  {isCurrent && <span className="stasis-check">✓</span>}
                </button>
              );
            })}
          </div>
        </div>
      </Popover>
    </>
  );
}
