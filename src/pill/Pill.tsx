import { useCallback, useEffect, useRef, useState, type MouseEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { events, type UnlistenFn } from "../shared/api";
import type { AppUiState } from "../shared/types";

const SUCCESS_MS = 280;

const BAR_COUNT = 5;
// Bell curve so the middle bar leads, like a voice-assistant waveform.
const BAR_WEIGHTS = [0.55, 0.85, 1, 0.85, 0.55];

type OverlayMode = "idle" | "recording" | "processing" | "success";

function toMode(state: AppUiState): OverlayMode {
  if (state === "recording" || state === "processing") return state;
  return "idle";
}

export function Pill() {
  const [mode, setMode] = useState<OverlayMode>("idle");

  const coreRef = useRef<HTMLDivElement | null>(null);
  const ringARef = useRef<HTMLSpanElement | null>(null);
  const ringBRef = useRef<HTMLSpanElement | null>(null);
  const barRefs = useRef<(HTMLSpanElement | null)[]>([]);
  const barVals = useRef<number[]>(new Array<number>(BAR_COUNT).fill(0.2));
  const levelRef = useRef(0);
  const envRef = useRef(0);
  const successTimer = useRef<number | null>(null);
  const flashingRef = useRef(false);

  const show = useCallback((next: OverlayMode) => {
    if (successTimer.current !== null) {
      window.clearTimeout(successTimer.current);
      successTimer.current = null;
    }
    flashingRef.current = next === "success";
    setMode(next);
  }, []);

  const rest = useCallback((delay: number) => {
    if (successTimer.current !== null) window.clearTimeout(successTimer.current);
    successTimer.current = window.setTimeout(() => {
      successTimer.current = null;
      flashingRef.current = false;
      setMode("idle");
    }, delay);
  }, []);

  useEffect(() => {
    let alive = true;
    const unlisten: UnlistenFn[] = [];

    const track = async (promise: Promise<UnlistenFn>) => {
      const fn = await promise;
      if (!alive) {
        fn();
        return;
      }
      unlisten.push(fn);
    };

    void track(
      events.appState((p) => {
        const next = toMode(p.state);
        if (next !== "idle") {
          show(next);
        } else if (!flashingRef.current) {
          rest(0);
        }
      }),
    );
    void track(
      events.audioLevel((p) => {
        const level = Number.isFinite(p.level) ? p.level : 0;
        levelRef.current = Math.max(0, Math.min(1, level));
      }),
    );
    void track(events.recordingStarted(() => show("recording")));
    void track(events.processing(() => show("processing")));
    void track(
      events.done(() => {
        show("success");
        rest(SUCCESS_MS);
      }),
    );
    void track(
      events.error(() => {
        flashingRef.current = false;
        rest(0);
      }),
    );

    return () => {
      alive = false;
      unlisten.forEach((fn) => {
        void fn();
      });
      if (successTimer.current !== null) window.clearTimeout(successTimer.current);
    };
  }, [rest, show]);

  const live = mode === "recording";
  const active = mode !== "idle";

  useEffect(() => {
    if (!live) {
      levelRef.current = 0;
      envRef.current = 0;
      barVals.current.fill(0.2);
      if (coreRef.current) coreRef.current.style.transform = "";
      return;
    }

    let frame = 0;
    const tick = (now: number) => {
      levelRef.current *= 0.965;
      const raw = Math.min(1, levelRef.current * 1.35);
      const env = envRef.current;
      envRef.current = env + (raw - env) * (raw > env ? 0.48 : 0.1);

      const idle = 0.1 + 0.06 * (0.5 + 0.5 * Math.sin(now * 0.0034));
      const speak = Math.max(idle, envRef.current);

      const ringA = ringARef.current;
      if (ringA) {
        ringA.style.transform = `scale(${(1.08 + speak * 0.42).toFixed(3)})`;
        ringA.style.opacity = (0.16 + speak * 0.42).toFixed(3);
      }
      const ringB = ringBRef.current;
      if (ringB) {
        ringB.style.transform = `scale(${(1.22 + speak * 0.7).toFixed(3)})`;
        ringB.style.opacity = (0.06 + speak * 0.28).toFixed(3);
      }
      const core = coreRef.current;
      if (core) {
        core.style.transform = `scale(${(1 + envRef.current * 0.045).toFixed(3)})`;
      }

      for (let i = 0; i < BAR_COUNT; i++) {
        const bar = barRefs.current[i];
        if (!bar) continue;
        const shimmer = 0.18 + 0.1 * (0.5 + 0.5 * Math.sin(now * 0.004 + i * 1.1));
        const target = Math.max(shimmer, envRef.current * BAR_WEIGHTS[i]);
        const prev = barVals.current[i];
        const next = prev + (target - prev) * (target > prev ? 0.5 : 0.22);
        barVals.current[i] = next;
        bar.style.transform = `scaleY(${next.toFixed(3)})`;
      }

      frame = requestAnimationFrame(tick);
    };

    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [live]);

  const onDrag = async (event: MouseEvent) => {
    if (mode !== "idle" || event.button !== 0) return;
    event.preventDefault();
    try {
      await getCurrentWindow().startDragging();
    } catch {
      // Overlay stays put if the window cannot drag.
    }
  };

  const label =
    mode === "recording"
      ? "Listening"
      : mode === "processing"
        ? "Transcribing"
        : mode === "success"
          ? "Inserted"
          : "Fuk";

  return (
    <div className="flex h-screen w-screen items-center justify-center overflow-hidden bg-transparent">
      <div
        className={`relative flex items-center justify-center ${
          active ? "size-full" : "size-full"
        }`}
        role="status"
        aria-live="polite"
        aria-label={label}
      >
        {mode === "recording" ? (
          <>
            <span
              ref={ringBRef}
              className="pointer-events-none absolute size-11 rounded-full border border-red/30"
              style={{ opacity: 0.08, transform: "scale(1.22)" }}
            />
            <span
              ref={ringARef}
              className="pointer-events-none absolute size-11 rounded-full border border-red/50"
              style={{ opacity: 0.18, transform: "scale(1.08)" }}
            />
          </>
        ) : null}

        {mode === "processing" ? (
          <svg
            viewBox="0 0 48 48"
            className="pointer-events-none absolute size-[52px] origin-center animate-loader text-red"
            aria-hidden
          >
            <circle
              cx="24"
              cy="24"
              r="20"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.25"
              strokeLinecap="round"
              strokeDasharray="18 108"
            />
          </svg>
        ) : null}

        <div
          ref={coreRef}
          onMouseDown={(event) => void onDrag(event)}
          className={`speak-orb relative z-10 flex items-center justify-center rounded-full transition-[width,height] duration-200 ease-out ${
            active ? "size-11" : "size-7 cursor-grab active:cursor-grabbing"
          } ${mode === "success" ? "text-green" : "text-paper"}`}
        >
          {mode === "idle" ? <FootballIcon /> : null}
          {mode === "recording" ? (
            <span className="flex h-[18px] items-center gap-[2.5px]" aria-hidden>
              {BAR_WEIGHTS.map((_, i) => (
                <span
                  key={i}
                  ref={(el) => {
                    barRefs.current[i] = el;
                  }}
                  className="h-full w-[2.5px] origin-center rounded-full bg-red"
                  style={{ transform: "scaleY(0.2)", willChange: "transform" }}
                />
              ))}
            </span>
          ) : null}
          {mode === "processing" ? (
            <span className="size-1.5 rounded-full bg-red/90" />
          ) : null}
          {mode === "success" ? <CheckIcon /> : null}
        </div>
      </div>
    </div>
  );
}

function FootballIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" aria-hidden>
      <circle cx="12" cy="12" r="9.2" fill="none" stroke="currentColor" strokeWidth="1.4" />
      <path
        d="M12 7.2 14.6 9.1 13.6 12.2h-3.2L9.4 9.1Z"
        fill="#e0361a"
      />
      <path
        d="M12 7.2 14.6 9.1l2.2-1.4M14.6 9.1 13.6 12.2l2.8 1.2M13.6 12.2 12 16.4M10.4 12.2 12 16.4 8.6 17.6M10.4 12.2 9.4 9.1 7.2 7.7M9.4 9.1 7.2 7.7"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.15"
        strokeLinejoin="round"
      />
      <path
        d="M16.8 7.7 19 10.4l-2.6 3M7.2 7.7 5 10.4l2.6 3M12 16.4l1.4 3.2M12 16.4l-1.4 3.2"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.15"
        strokeLinecap="round"
      />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-[18px] w-[18px] animate-popIn" aria-hidden>
      <path
        d="M6.5 12.4 10.2 16 17.5 8.2"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
