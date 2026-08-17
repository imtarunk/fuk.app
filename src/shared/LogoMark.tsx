export function LogoMark({ className = "h-9" }: { className?: string }) {
  return (
    <span
      className={`inline-flex items-center border border-ink bg-card px-2 shadow-press-sm ${className}`}
      aria-hidden
    >
      <svg viewBox="0 0 48 24" className="h-5 w-10" role="img">
        <title>fuk logo — a red waveform from voice to text</title>
        <circle cx="4" cy="12" r="2.2" fill="#171310" />
        <path
          d="M6.4 12h4.2l2.2-5 2.6 10 3-12 3 14 2.4-7H41.5"
          fill="none"
          stroke="#e0361a"
          strokeWidth="2.4"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <rect x="42.2" y="10.2" width="3.6" height="3.6" fill="#171310" />
      </svg>
    </span>
  );
}
