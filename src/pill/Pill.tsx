import { useCallback, useEffect, useRef, useState } from "react";
import { events, type UnlistenFn } from "../shared/api";
import type { AppUiState } from "../shared/types";

const BAR_COUNT = 11;
const BAR_MAX_PX = 18;
const BAR_FLOOR = 0.12;

const SUCCESS_MS = 200;
const LEAVE_MS = 120;

type PillContent = "recording" | "processing" | "success";

const BAR_WEIGHTS = Array.from({ length: BAR_COUNT }, (_, i) => {
  const bell = Math.sin((Math.PI * (i + 0.5)) / BAR_COUNT);
  const wobble = 0.94 + ((i * 7) % 5) * 0.024;
  return (0.42 + 0.58 * bell) * wobble;
});

function toContent(state: AppUiState): PillContent | null {
  if (state === "recording" || state === "processing") return state;
  return null;
}

export function Pill() {
  const [content, setContent] = useState<PillContent | null>(null);
  const [leaving, setLeaving] = useState(false);

  const barsRef = useRef<(HTMLSpanElement | null)[]>([]);
  const levelRef = useRef(0);
  const envRef = useRef(0);
  const valuesRef = useRef<number[]>(new Array(BAR_COUNT).fill(BAR_FLOOR));
  const velocitiesRef = useRef<number[]>(new Array(BAR_COUNT).fill(0));

  const leaveTimer = useRef<number | null>(null);
  const unmountTimer = useRef<number | null>(null);
  const flashingRef = useRef(false);

  const show = useCallback((next: PillContent) => {
    if (leaveTimer.current !== null) {
      window.clearTimeout(leaveTimer.current);
      leaveTimer.current = null;
    }
    if (unmountTimer.current !== null) {
      window.clearTimeout(unmountTimer.current);
      unmountTimer.current = null;
    }
    flashingRef.current = next === "success";
    setLeaving(false);
    setContent(next);
  }, []);

  const dismiss = useCallback((delay: number) => {
    if (leaveTimer.current !== null) window.clearTimeout(leaveTimer.current);
    leaveTimer.current = window.setTimeout(() => {
      leaveTimer.current = null;
      setLeaving(true);
      unmountTimer.current = window.setTimeout(() => {
        unmountTimer.current = null;
        flashingRef.current = false;
        setLeaving(false);
        setContent(null);
      }, LEAVE_MS);
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
        const next = toContent(p.state);
        if (next) {
          show(next);
        } else if (!flashingRef.current) {
          dismiss(0);
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
        dismiss(SUCCESS_MS);
      }),
    );
    void track(
      events.error(() => {
        flashingRef.current = false;
        dismiss(0);
      }),
    );

    return () => {
      alive = false;
      unlisten.forEach((fn) => {
        void fn();
      });
      if (leaveTimer.current !== null) window.clearTimeout(leaveTimer.current);
      if (unmountTimer.current !== null)
        window.clearTimeout(unmountTimer.current);
    };
  }, [dismiss, show]);

  const live = content === "recording" && !leaving;

  useEffect(() => {
    if (!live) {
      levelRef.current = 0;
      envRef.current = 0;
      valuesRef.current.fill(BAR_FLOOR);
      velocitiesRef.current.fill(0);
      return;
    }

    let frame = 0;
    const tick = (now: number) => {
      levelRef.current *= 0.965;

      const raw = Math.min(1, levelRef.current * 1.3);
      const env = envRef.current;
      envRef.current = env + (raw - env) * (raw > env ? 0.5 : 0.12);

      const values = valuesRef.current;
      const velocities = velocitiesRef.current;

      for (let i = 0; i < BAR_COUNT; i += 1) {
        const shimmer = 0.11 + 0.06 * Math.sin(now * 0.0038 + i * 0.62);
        const target = Math.max(shimmer, envRef.current * BAR_WEIGHTS[i]);

        velocities[i] = (velocities[i] + (target - values[i]) * 0.34) * 0.62;
        const next = Math.max(BAR_FLOOR, Math.min(1, values[i] + velocities[i]));
        values[i] = next;

        const el = barsRef.current[i];
        if (el) {
          el.style.transform = `scaleY(${next.toFixed(3)})`;
        }
      }

      frame = requestAnimationFrame(tick);
    };

    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [live]);

  if (content === null) {
    return <div className="h-px w-px bg-transparent" />;
  }

  return (
    <div className="flex h-screen w-screen items-center justify-center overflow-hidden bg-transparent p-[4px]">
      <div
        className={`pointer-events-none relative flex h-11 w-[200px] select-none items-center justify-center gap-2.5 border border-ink bg-card px-3 shadow-press ${
          leaving ? "animate-pillOut" : "animate-pillIn"
        }`}
      >
        {content === "recording" ? (
          <>
            <span className="relative flex size-2.5 shrink-0 items-center justify-center">
              <span className="size-2 rounded-full bg-red animate-breathe" />
            </span>
            <div className="flex h-[18px] items-center gap-[4px]">
              {BAR_WEIGHTS.map((_, i) => (
                <span
                  key={i}
                  ref={(el) => {
                    barsRef.current[i] = el;
                  }}
                  className="w-[3px] origin-center bg-red"
                  style={{
                    height: `${BAR_MAX_PX}px`,
                    transform: `scaleY(${BAR_FLOOR})`,
                    willChange: "transform",
                  }}
                />
              ))}
            </div>
          </>
        ) : null}

        {content === "processing" ? (
          <>
            <span className="h-3 w-[2px] bg-red animate-caret-blink" />
            <span className="font-mono text-[11px] font-medium uppercase tracking-[0.12em] text-ink">
              Transcribing
            </span>
          </>
        ) : null}

        {content === "success" ? (
          <span className="flex animate-popIn items-center gap-2">
            <span className="flex size-4 items-center justify-center bg-green">
              <svg viewBox="0 0 24 24" className="h-3 w-3 text-white" aria-hidden>
                <path
                  d="M5 13l4 4L19 7"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="3"
                />
              </svg>
            </span>
            <span className="font-mono text-[11px] font-medium uppercase tracking-[0.12em] text-ink">
              Inserted
            </span>
          </span>
        ) : null}
      </div>
    </div>
  );
}
