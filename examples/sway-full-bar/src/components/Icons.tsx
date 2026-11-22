export function SpeakerIcon({
  volume,
  muted,
}: {
  volume: number;
  muted: boolean;
}) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="currentColor"
      aria-hidden="true"
    >
      <path d="M2 5.5v5h3l4 3.5V2L5 5.5H2z" />
      {muted ? (
        <path
          d="M11 5.5l3.5 5M14.5 5.5L11 10.5"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
          fill="none"
        />
      ) : (
        <>
          {volume > 0 && (
            <path
              d="M11 5.8a3.2 3.2 0 0 1 0 4.4"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              fill="none"
            />
          )}
          {volume >= 0.4 && (
            <path
              d="M12.8 3.8a6 6 0 0 1 0 8.4"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              fill="none"
            />
          )}
        </>
      )}
    </svg>
  );
}

export function DisplayIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      aria-hidden="true"
    >
      <rect x="1.75" y="2.5" width="12.5" height="8.5" rx="1.2" />
      <path d="M5.5 13.5h5M8 11v2.5" strokeLinecap="round" />
    </svg>
  );
}

export function BluetoothIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M4.5 5l7 6-3.5 3V2l3.5 3-7 6" />
    </svg>
  );
}

export function IdleIcon({ inhibited }: { inhibited: boolean }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12.5 9.8A5.2 5.2 0 0 1 6.2 3.5a5.2 5.2 0 1 0 6.3 6.3z" />
      {inhibited && <path d="M2.5 13.5l11-11" />}
    </svg>
  );
}

export function MicIcon({ muted }: { muted: boolean }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="5.5" y="2" width="5" height="7.5" rx="2.5" />
      <path d="M3.5 7.5a4.5 4.5 0 0 0 9 0M8 12v2.2M5.5 14.2h5" />
      {muted && <path d="M2.5 13.5l11-11" strokeWidth="1.6" />}
    </svg>
  );
}

export function MixerIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <path d="M3 4h10M3 8h10M3 12h10" />
      <circle cx="5.5" cy="4" r="1.5" fill="currentColor" />
      <circle cx="10.5" cy="8" r="1.5" fill="currentColor" />
      <circle cx="7" cy="12" r="1.5" fill="currentColor" />
    </svg>
  );
}

export function SunIcon() {
  return (
    <svg
      width="15"
      height="15"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <circle cx="8" cy="8" r="3" />
      <path d="M8 1.75v1.5M8 12.75v1.5M1.75 8h1.5M12.75 8h1.5M3.58 3.58l1.06 1.06M11.36 11.36l1.06 1.06M12.42 3.58l-1.06 1.06M4.64 11.36l-1.06 1.06" />
    </svg>
  );
}

export function MoonIcon() {
  return (
    <svg
      width="15"
      height="15"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12.5 9.8A5.2 5.2 0 0 1 6.2 3.5a5.2 5.2 0 1 0 6.3 6.3z" />
    </svg>
  );
}

export function BellIcon({ dnd }: { dnd: boolean }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M4.2 6.6a3.8 3.8 0 0 1 7.6 0c0 2.6 1 3.7 1.7 4.4H2.5c.7-.7 1.7-1.8 1.7-4.4z" />
      <path d="M6.6 13a1.6 1.6 0 0 0 2.8 0" />
      {dnd && <path d="M2.5 13.5l11-11" />}
    </svg>
  );
}
