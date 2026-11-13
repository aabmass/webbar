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
