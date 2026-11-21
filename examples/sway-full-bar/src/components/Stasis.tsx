import { useCallback, useEffect, useRef, useState } from "react";
import {
  DEFAULT_PROFILE,
  extractBlockers,
  formatProfileLabel,
  parseProfilesOutput,
  type BlameSnapshot,
  type StasisWatchState,
} from "@webbar/sway-full-bar-shared/stasis";
import { IdleIcon } from "./Icons";
import { Popover } from "./Popover";

export function Stasis() {
  const [status, setStatus] = useState<StasisWatchState | null>(null);
  const [profiles, setProfiles] = useState<string[]>([DEFAULT_PROFILE]);
  const [blame, setBlame] = useState<BlameSnapshot | null>(null);
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
      const res = await window.webbar.exec(["stasis", "blame", "--json"]);
      if (res.code === 0) {
        const parsed = JSON.parse(res.stdout) as BlameSnapshot;
        setBlame(parsed);
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
  const isManuallyPaused = Boolean(status?.manually_paused);
  const blockers = extractBlockers(blame);
  const hasExternalInhibitor = Boolean(
    status?.paused || blockers.length > 0,
  );
  const isInhibited = isManuallyPaused || hasExternalInhibitor;

  useEffect(() => {
    if (!status) {
      setBlame(null);
      return;
    }
    void refreshBlame();
    const timer = window.setInterval(() => {
      void refreshBlame();
    }, 3000);
    return () => {
      window.clearInterval(timer);
    };
  }, [status, refreshBlame]);

  useEffect(() => {
    if (!open) return;
    void refreshProfiles();
    void refreshBlame();
  }, [open, refreshProfiles, refreshBlame]);

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
    void runStasisCommand(["stasis", isManuallyPaused ? "resume" : "pause"]);
  }, [isManuallyPaused, runStasisCommand]);

  const statusSuffix = isManuallyPaused
    ? " (paused)"
    : isInhibited
      ? " (inhibited)"
      : "";

  const badgeTooltip = !status
    ? "Stasis idle daemon is not running or unreachable"
    : isManuallyPaused
      ? "Idle timers are manually paused until resumed"
      : isInhibited
        ? blockers.length > 0
          ? `Idle progression blocked by: ${blockers.map((b) => b.name).join(", ")}`
          : "Idle progression is blocked by an active inhibitor"
        : `Idle timers are running (${formatProfileLabel(activeProfile)} profile)`;

  const pillClass = [
    "bar-btn",
    "stasis-btn",
    !status ? "is-dimmed" : "",
    isInhibited ? "is-inhibited" : "",
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
              ? `Idle: ${formatProfileLabel(activeProfile)}${statusSuffix} — click for controls, right-click to ${isManuallyPaused ? "resume" : "pause"}`
              : "Stasis offline"
        }
        onClick={() => setOpen((prev) => !prev)}
        onContextMenu={(e) => {
          e.preventDefault();
          togglePause();
        }}
      >
        <IdleIcon inhibited={isInhibited} />
        <span className="stasis-label">
          {status ? formatProfileLabel(activeProfile) : "idle"}
        </span>
      </button>

      <Popover open={open} onClose={close} anchorRef={btnRef} align="end">
        <div className="stasis-popover">
          <div className="stasis-header">
            <span className="stasis-title">Idle</span>
            {!status ? (
              <span className="stasis-badge is-offline" title={badgeTooltip}>
                offline
              </span>
            ) : isInhibited ? (
              <span className="stasis-badge is-inhibited" title={badgeTooltip}>
                {isManuallyPaused ? "paused" : "inhibited"}
              </span>
            ) : (
              <span className="stasis-badge" title={badgeTooltip}>
                active
              </span>
            )}
          </div>

          <button
            type="button"
            className={`bar-btn stasis-action-btn${isManuallyPaused ? " is-active" : ""}`}
            disabled={busy || !status}
            onClick={togglePause}
          >
            {isManuallyPaused ? "Resume Idle" : "Pause Idle"}
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

          {blockers.length > 0 && (
            <>
              <div className="stasis-section-label">Inhibitors</div>
              <div className="stasis-inhibitor-list">
                {blockers.map((item) => (
                  <div key={item.key} className="stasis-inhibitor-item">
                    <span className="stasis-inhibitor-name">{item.name}</span>
                    {item.detail && (
                      <span className="stasis-inhibitor-detail">
                        {item.detail}
                      </span>
                    )}
                  </div>
                ))}
              </div>
            </>
          )}

          {error && <div className="stasis-error">{error}</div>}
        </div>
      </Popover>
    </>
  );
}
