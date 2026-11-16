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
  const [blame, setBlame] = useState<string | null>(null);
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

  const refreshBlame = useCallback(async () => {
    try {
      const res = await window.webbar.exec(["stasis", "blame"]);
      if (res.code === 0) {
        const text = res.stdout.trim();
        setBlame(text && text !== "No current blockers" ? text : null);
      }
    } catch {
      setBlame(null);
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

  const activeProfile = status?.profile ?? DEFAULT_PROFILE;
  const isPaused = Boolean(status?.paused || status?.manually_paused);

  useEffect(() => {
    if (!open) return;
    void refreshProfiles();
    if (isPaused) {
      void refreshBlame();
    } else {
      setBlame(null);
    }
  }, [open, isPaused, refreshProfiles, refreshBlame]);

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

  const togglePause = useCallback(() => {
    void runStasisCommand(["stasis", isPaused ? "resume" : "pause"]);
  }, [isPaused, runStasisCommand]);

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
              ? `Idle: ${formatProfileLabel(activeProfile)}${isPaused ? " (paused)" : ""} — click for controls, right-click to ${isPaused ? "resume" : "pause"}`
              : "Stasis offline"
        }
        onClick={() => setOpen((prev) => !prev)}
        onContextMenu={(e) => {
          e.preventDefault();
          togglePause();
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
            <span className="stasis-title">Idle</span>
            {!status ? (
              <span className="stasis-badge is-offline">offline</span>
            ) : (
              isPaused && (
                <span className="stasis-badge is-inhibited">paused</span>
              )
            )}
          </div>

          {error && <div className="stasis-error">{error}</div>}
          {blame && <div className="stasis-blame">{blame}</div>}

          <button
            type="button"
            className={`bar-btn stasis-action-btn${isPaused ? " is-active" : ""}`}
            disabled={busy || !status}
            onClick={togglePause}
          >
            {isPaused ? "Resume Idle" : "Pause Idle"}
          </button>

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
