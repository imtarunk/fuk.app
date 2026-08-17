import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { api, events, type UnlistenFn } from "../shared/api";
import { LogoMark } from "../shared/LogoMark";
import {
  DEFAULT_CONFIG,
  LLM_MODELS,
  WHISPER_MODELS,
  type AppConfig,
  type AppMode,
  type AudioDevice,
  type DownloadProgressPayload,
  type InsertMode,
  type ModelStatus,
  type PermissionsStatus,
} from "../shared/types";

type Tone = "ok" | "warn" | "busy";

const MODIFIER_KEYS = new Set(["Control", "Alt", "Shift", "Meta"]);

const LONE_MODIFIER_BY_CODE: Record<string, string> = {
  ControlLeft: "ControlLeft",
  ControlRight: "ControlRight",
  AltLeft: "Alt",
  AltRight: "AltGr",
  ShiftLeft: "ShiftLeft",
  ShiftRight: "ShiftRight",
  MetaLeft: "MetaLeft",
  MetaRight: "MetaRight",
  OSLeft: "MetaLeft",
  OSRight: "MetaRight",
};

const LONE_MODIFIER_BY_KEY: Record<string, string> = {
  Control: "ControlLeft",
  Alt: "Alt",
  Shift: "ShiftLeft",
  Meta: "MetaLeft",
};

const KEYCAP_LABELS: Record<string, string> = {
  Control: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  Command: "⌘",
  Meta: "⌘",
  ControlLeft: "⌃ L",
  ControlRight: "⌃ R",
  AltLeft: "⌥ L",
  AltRight: "⌥ R",
  AltGr: "⌥ R",
  ShiftLeft: "⇧ L",
  ShiftRight: "⇧ R",
  MetaLeft: "⌘ L",
  MetaRight: "⌘ R",
  Function: "fn",
  Space: "Space",
  CapsLock: "caps",
  Tab: "⇥",
  Escape: "esc",
  Backspace: "⌫",
  Enter: "↩",
  Return: "↩",
  Minus: "-",
  Equal: "=",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  Quote: "'",
  Semicolon: ";",
  Backquote: "`",
  BracketLeft: "[",
  BracketRight: "]",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
};

function hotkeyFromEvent(event: KeyboardEvent): string | null {
  if (event.key === "Fn" || event.key === "Function" || event.code === "Fn") {
    return "Function";
  }

  if (MODIFIER_KEYS.has(event.key)) {
    return (
      LONE_MODIFIER_BY_CODE[event.code] ??
      LONE_MODIFIER_BY_KEY[event.key] ??
      null
    );
  }

  const code = event.code;
  if (!code) return null;

  const tokens: string[] = [];
  if (event.ctrlKey) tokens.push("Control");
  if (event.altKey) tokens.push("Alt");
  if (event.shiftKey) tokens.push("Shift");
  if (event.metaKey) tokens.push("Command");

  if (tokens.length === 0 && !/^F\d{1,2}$/.test(code)) return null;

  tokens.push(code);
  return tokens.join("+");
}

function keycapLabel(token: string): string {
  const direct = KEYCAP_LABELS[token];
  if (direct) return direct;
  if (/^Key[A-Z]$/.test(token)) return token.slice(3);
  if (/^Digit\d$/.test(token)) return token.slice(5);
  if (/^F\d{1,2}$/.test(token)) return token;
  if (token.length === 1) return token.toUpperCase();
  return token.replace(/([a-z])([A-Z])/g, "$1 $2");
}

function hotkeyTokens(hotkey: string): string[] {
  return hotkey
    .split("+")
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

function errorMessage(err: unknown): string {
  if (err instanceof Error && err.message) return err.message;
  if (typeof err === "string" && err.trim()) return err;
  return "Something went wrong.";
}

export function Settings() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [status, setStatus] = useState<ModelStatus | null>(null);
  const [perms, setPerms] = useState<PermissionsStatus | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [banner, setBanner] = useState<string | null>(null);
  const [permissionNote, setPermissionNote] = useState<string | null>(null);
  const [progress, setProgress] = useState<DownloadProgressPayload | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [justBound, setJustBound] = useState(false);
  const captureRef = useRef<HTMLDivElement | null>(null);
  const boundTimer = useRef<number | null>(null);
  const autoDownload = useRef(false);

  const flashBound = useCallback(() => {
    if (boundTimer.current !== null) window.clearTimeout(boundTimer.current);
    setJustBound(true);
    boundTimer.current = window.setTimeout(() => {
      boundTimer.current = null;
      setJustBound(false);
    }, 1400);
  }, []);

  useEffect(
    () => () => {
      if (boundTimer.current !== null) window.clearTimeout(boundTimer.current);
    },
    [],
  );

  const refreshModels = useCallback(async () => {
    try {
      const next = await api.getModelStatus();
      setStatus(next);
      if (next.ready_for_fast && next.ready_for_polish) {
        setDownloading(false);
        setProgress(null);
      }
    } catch (err) {
      setBanner(errorMessage(err));
    }
  }, []);

  const persist = useCallback(async (next: AppConfig) => {
    try {
      setBanner(null);
      setConfig(await api.saveConfig(next));
    } catch (err) {
      setBanner(errorMessage(err));
    }
  }, []);

  const startDownload = useCallback(async () => {
    try {
      setBanner(null);
      setDownloading(true);
      await api.startModelDownload();
      await refreshModels();
      setDownloading(false);
      setProgress(null);
    } catch (err) {
      setDownloading(false);
      setBanner(errorMessage(err));
    }
  }, [refreshModels]);

  useEffect(() => {
    let alive = true;

    const load = async () => {
      try {
        const cfg = await api.getConfig();
        if (alive) setConfig(cfg);
      } catch (err) {
        if (!alive) return;
        setBanner(errorMessage(err));
        setConfig(DEFAULT_CONFIG);
      }

      try {
        const models = await api.getModelStatus();
        if (alive) setStatus(models);
      } catch (err) {
        if (alive) setBanner(errorMessage(err));
      }

      try {
        const permissions = await api.checkPermissions();
        if (alive) setPerms(permissions);
      } catch (err) {
        if (alive) setBanner(errorMessage(err));
      }

      try {
        const list = await api.listInputDevices();
        if (alive) setDevices(list);
      } catch (err) {
        if (alive) setBanner(errorMessage(err));
      }
    };

    void load();
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (!status) return;
    if (status.ready_for_fast && status.ready_for_polish) return;
    if (downloading || autoDownload.current) return;
    autoDownload.current = true;
    void startDownload();
  }, [downloading, startDownload, status]);

  useEffect(() => {
    const onFocus = () => {
      void api
        .checkPermissions()
        .then(setPerms)
        .catch((err: unknown) => setBanner(errorMessage(err)));
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
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
      events.downloadProgress((p) => {
        setProgress(p);
        setDownloading(true);
        if (p.total > 0 && p.downloaded >= p.total) {
          void refreshModels();
        }
      }),
    );
    void track(
      events.error((p) => {
        setBanner(p.message);
        setDownloading(false);
      }),
    );
    void track(
      events.permissionNeeded((p) => {
        setPermissionNote(p.message);
      }),
    );
    void track(
      events.configUpdated((next) => {
        setConfig(next);
      }),
    );
    void track(
      events.hotkeyCaptured(async () => {
        setCapturing(false);
        flashBound();
        try {
          setConfig(await api.getConfig());
        } catch (err) {
          setBanner(errorMessage(err));
        }
      }),
    );

    return () => {
      alive = false;
      unlisten.forEach((fn) => {
        void fn();
      });
    };
  }, [flashBound, refreshModels]);

  const patch = (partial: Partial<AppConfig>) => {
    if (!config) return;
    void persist({ ...config, ...partial });
  };

  const enableMic = async () => {
    try {
      setBanner(null);
      setPerms(await api.requestPermissions());
    } catch (err) {
      setBanner(errorMessage(err));
    }
  };

  const openAccessibility = async () => {
    try {
      setBanner(null);
      // Recheck only re-arms. Never ask macOS to prompt — that dialog fires
      // even when Accessibility is already on after a rebuild.
      const next = await api.requestAccessibility();
      setPerms(next);
      const stillBlocked =
        next.hotkey_needs_accessibility && !next.hotkey_listening;
      if (!next.accessibility || stillBlocked) {
        await api.openAccessibilitySettings();
      }
    } catch (err) {
      setBanner(errorMessage(err));
    }
  };

  const startCapture = async () => {
    try {
      setBanner(null);
      await api.beginHotkeyCapture();
      setCapturing(true);
    } catch (err) {
      setBanner(errorMessage(err));
    }
  };

  const cancelCapture = useCallback(async () => {
    try {
      await api.cancelHotkeyCapture();
    } catch (err) {
      setBanner(errorMessage(err));
    } finally {
      setCapturing(false);
    }
  }, []);

  useEffect(() => {
    if (!capturing) return;
    captureRef.current?.focus();

    const onKey = (event: KeyboardEvent) => {
      if (event.repeat) return;
      event.preventDefault();
      event.stopPropagation();

      if (event.key === "Escape") {
        void cancelCapture();
        return;
      }

      const hotkey = hotkeyFromEvent(event);
      if (!hotkey) return;

      void (async () => {
        try {
          const applied = await api.setHotkey(hotkey);
          setConfig((prev) =>
            prev ? { ...prev, hotkey: applied || hotkey } : prev,
          );
          flashBound();
          setPerms(await api.checkPermissions());
        } catch (err) {
          setBanner(errorMessage(err));
        } finally {
          setCapturing(false);
        }
      })();
    };

    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [cancelCapture, capturing, flashBound]);

  const percent =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  const hotkey = config?.hotkey ?? DEFAULT_CONFIG.hotkey;
  const hotkeyNeedsAccessibility =
    perms?.hotkey_needs_accessibility ?? !hotkey.includes("+");
  const accessibilityOk = perms?.accessibility ?? false;
  const micOk = perms?.microphone ?? false;

  const fastReady = status?.ready_for_fast ?? false;
  const polishReady = status?.ready_for_polish ?? false;
  const modelsReady = fastReady && polishReady;
  const llmEnabled = config?.mode === "polish";
  const polishModel =
    LLM_MODELS.find(
      (model) =>
        model.id === (config?.llm_model ?? DEFAULT_CONFIG.llm_model),
    ) ?? LLM_MODELS[0];
  const missing = status
    ? [...status.whisper, ...status.llm].filter(
        (file) => file.required && !file.present,
      )
    : [];

  const readiness: { label: string; tone: Tone } = (() => {
    if (downloading) {
      return {
        label: percent === null ? "Downloading" : `Downloading ${percent}%`,
        tone: "busy",
      };
    }
    if (!status || !perms) return { label: "Checking", tone: "busy" };
    const blocked =
      !fastReady ||
      !micOk ||
      (hotkeyNeedsAccessibility && !accessibilityOk);
    return blocked
      ? { label: "Needs setup", tone: "warn" }
      : { label: "Ready", tone: "ok" };
  })();

  return (
    <div className="scroll-slim h-screen overflow-y-auto bg-paper text-ink-soft">
      <div className="dot-grid mx-auto flex w-full max-w-[472px] flex-col px-6 pb-14 pt-7">
        <header className="mb-8 flex items-start justify-between gap-4">
          <div className="min-w-0">
            <Eyebrow>Settings</Eyebrow>
            <div className="mt-3 flex items-center gap-3">
              <LogoMark />
              <h1 className="font-display text-[30px] font-semibold leading-[1.1] tracking-[-0.02em] text-ink">
                fuk
              </h1>
            </div>
            <p className="mt-3 max-w-[34ch] font-sans text-[15px] leading-[1.65]">
              Hold a key, speak, get{" "}
              <em className="font-serif italic text-red">text</em>. Audio never
              leaves this Mac.
            </p>
          </div>
          <StatusChip tone={readiness.tone} label={readiness.label} />
        </header>

        {banner ? <Banner tone="error">{banner}</Banner> : null}
        {permissionNote ? <Banner tone="warn">{permissionNote}</Banner> : null}

        {modelsReady ? (
          <div className="mb-8 flex items-center justify-between gap-3 border border-ink bg-card px-4 py-3 shadow-press">
            <div className="flex items-center gap-2.5">
              <Mark tone="ok" />
              <span className="font-mono text-[13px] font-medium uppercase tracking-[0.08em] text-ink">
                Models ready
              </span>
            </div>
            <span className="font-mono text-[11px] font-medium uppercase tracking-[0.12em] text-mute">
              Offline
            </span>
          </div>
        ) : (
          <div className="mb-8 border border-ink bg-card p-6 shadow-press-lg">
            <Eyebrow>First run</Eyebrow>
            <h2 className="mt-3 font-display text-[20px] font-semibold tracking-[-0.01em] text-ink">
              {downloading ? "Downloading models" : "Preparing models"}
            </h2>
            <p className="mt-2 font-sans text-[15px] leading-[1.65]">
              Fuk picks a polish model for this Mac and fetches Whisper plus the
              LLM from Hugging Face in the background. After that it runs fully
              offline.
            </p>
            {missing.length > 0 ? (
              <ul className="mt-3 flex flex-wrap gap-1.5">
                {missing.map((file) => (
                  <li
                    key={file.id}
                    className="border border-ink bg-paper px-1.5 py-0.5 font-mono text-[11px] uppercase tracking-[0.08em] text-ink-soft"
                  >
                    {file.filename}
                  </li>
                ))}
              </ul>
            ) : null}
            {banner && !downloading ? (
              <Btn
                variant="accent"
                className="mt-5"
                onClick={() => {
                  autoDownload.current = false;
                  void startDownload();
                }}
              >
                Retry download
              </Btn>
            ) : null}
            {progress ? (
              <div className="mt-5">
                <div className="mb-1.5 flex items-baseline justify-between gap-3 font-mono text-[11px] uppercase tracking-[0.12em]">
                  <span className="truncate text-mute">{progress.model}</span>
                  <span className="tabular-nums text-faint">
                    {percent === null ? "…" : `${percent}%`}
                  </span>
                </div>
                <ProgressBar percent={percent} />
              </div>
            ) : downloading ? (
              <div className="mt-5">
                <ProgressBar percent={null} />
              </div>
            ) : null}
          </div>
        )}

        <Ruled>
          <Section label="Trigger">
            <div className="flex items-start justify-between gap-4">
              <div className="min-w-0">
                <h3 className="font-display text-[18px] font-semibold tracking-[-0.01em] text-ink">
                  Push-to-talk key
                </h3>
                <p className="mt-1 font-sans text-[15px] leading-[1.65]">
                  Hold to record, release to transcribe and insert.
                </p>
              </div>
              {capturing ? (
                <Btn onClick={() => void cancelCapture()}>Cancel</Btn>
              ) : (
                <Btn onClick={() => void startCapture()}>Change</Btn>
              )}
            </div>

            {capturing ? (
              <div
                ref={captureRef}
                tabIndex={-1}
                className="mt-4 flex flex-col items-center gap-2.5 border border-dashed border-ink bg-paper-deep px-4 py-6 outline-none"
              >
                <div className="flex items-center gap-2">
                  <span className="h-2 w-2 bg-red animate-breathe" />
                  <span className="font-mono text-[13px] font-medium uppercase tracking-[0.08em] text-red">
                    Listening for keys
                  </span>
                </div>
                <p className="text-center font-sans text-[13px] leading-relaxed text-mute">
                  Press a combo such as <Keycap>⌥</Keycap> <Keycap>Space</Keycap>
                  , or a single modifier like <Keycap>⌃ R</Keycap>. Esc cancels.
                </p>
              </div>
            ) : (
              <div
                className={`mt-4 flex items-center gap-2 border px-3 py-2.5 ${
                  justBound
                    ? "border-red bg-card"
                    : "border-ink bg-paper-deep"
                }`}
              >
                <div className="flex flex-wrap items-center gap-1.5">
                  {hotkeyTokens(hotkey).map((token, i) => (
                    <Keycap key={`${token}-${i}`}>{keycapLabel(token)}</Keycap>
                  ))}
                </div>
                {justBound ? (
                  <span className="ml-auto font-mono text-[11px] font-medium uppercase tracking-[0.12em] text-red">
                    Saved
                  </span>
                ) : null}
              </div>
            )}

            {hotkeyNeedsAccessibility ? (
              <Note className="mt-4">
                Single keys need{" "}
                <span className="text-ink">Accessibility</span> and{" "}
                <span className="text-ink">Input Monitoring</span> so they
                still fire when another app is in front. A combo like{" "}
                <Keycap>⌥</Keycap> <Keycap>Space</Keycap> needs none.
                {perms && !perms.hotkey_listening ? (
                  <span className="mt-2 block">
                    If the toggle is already on, turn Fuk off and on again
                    after a rebuild, then Recheck.
                  </span>
                ) : null}
                {!accessibilityOk || (perms && !perms.hotkey_listening) ? (
                  <span className="mt-3 block">
                    <Btn onClick={() => void openAccessibility()}>
                      Open System Settings
                    </Btn>
                  </span>
                ) : null}
              </Note>
            ) : (
              <p className="mt-4 font-sans text-[13px] leading-relaxed text-mute">
                Combos are picked up system-wide without a permission prompt.
                {perms && !perms.hotkey_listening
                  ? " If nothing happens in other apps, quit and reopen Fuk."
                  : ""}
              </p>
            )}
          </Section>

          <Section label="Transcription">
            <Segmented<AppMode>
              value={config?.mode ?? DEFAULT_CONFIG.mode}
              options={[
                { id: "fast", label: "Fast" },
                { id: "polish", label: "Polish" },
              ]}
              ariaLabel="Transcription mode"
              onChange={(mode) => patch({ mode })}
            />
            <p className="mt-3 font-sans text-[13px] leading-[1.65] text-mute">
              Fast runs Whisper with rule-based cleanup. Polish adds a local LLM
              pass for punctuation and phrasing.
              {!polishReady
                ? " The polish model downloads automatically in the background."
                : ""}
            </p>

            <CardLabel>Speech model</CardLabel>
            <OptionGroup label="Whisper model">
              {WHISPER_MODELS.map((model) => (
                <OptionRow
                  key={model.id}
                  selected={
                    (config?.whisper_model ?? DEFAULT_CONFIG.whisper_model) ===
                    model.id
                  }
                  label={model.label}
                  hint={model.hint}
                  onSelect={() => patch({ whisper_model: model.id })}
                />
              ))}
            </OptionGroup>

            <CardLabel>
              Polish model
              <span className="ml-2 font-normal normal-case tracking-normal text-faint">
                Auto-selected
              </span>
            </CardLabel>
            <div className="border border-ink bg-paper-deep px-3 py-2.5">
              <p className="font-display text-[16px] font-semibold tracking-[-0.01em] text-ink">
                {polishModel.label}
              </p>
              <p className="mt-0.5 font-sans text-[13px] leading-relaxed text-mute">
                {status?.machine_tier === "higher"
                  ? "Higher-spec machine — Qwen3 0.6B Q4."
                  : "Lower-spec machine — SmolLM2 360M Q4."}{" "}
                {polishModel.hint}
                {!llmEnabled ? " Used in Polish mode." : ""}
              </p>
            </div>
          </Section>

          <Section label="Audio & output">
            <CardLabel>Microphone</CardLabel>
            <Select
              label="Input device"
              value={config?.input_device ?? ""}
              onChange={(value) =>
                patch({ input_device: value === "" ? null : value })
              }
              options={[
                { value: "", label: "System default" },
                ...devices.map((device) => ({
                  value: device.id,
                  label: device.is_default
                    ? `${device.name} (system default)`
                    : device.name,
                })),
              ]}
            />

            <CardLabel>Insert mode</CardLabel>
            <Segmented<InsertMode>
              value={config?.insert_mode ?? DEFAULT_CONFIG.insert_mode}
              options={[
                { id: "paste", label: "Paste at cursor" },
                { id: "clipboard_only", label: "Clipboard only" },
              ]}
              ariaLabel="Insert mode"
              onChange={(insert_mode) => patch({ insert_mode })}
            />
            {perms?.wayland && config?.insert_mode === "paste" ? (
              <p className="mt-3 font-sans text-[13px] leading-relaxed text-amber">
                Clipboard only is recommended on this display server.
              </p>
            ) : (
              <p className="mt-3 font-sans text-[13px] leading-relaxed text-mute">
                Fuk remembers the app you were in when you held the hotkey
                and pastes there when transcription finishes — you do not need
                to click back. Accessibility is required for the paste itself.
              </p>
            )}
          </Section>

          <Section label="Permissions">
            {perms?.wayland ? (
              <Note className="mb-4">
                Auto-insert may not work on this display server. Use
                clipboard-only insert mode.
              </Note>
            ) : null}

            <StatusRow
              ok={micOk}
              label="Microphone"
              detail={
                micOk ? "Allowed" : "Required to record anything at all."
              }
              actionLabel={micOk ? "Recheck" : "Allow access"}
              onAction={() => void enableMic()}
            />
            <div className="my-4 h-px bg-ink" />
            <StatusRow
              ok={
                accessibilityOk &&
                (!hotkeyNeedsAccessibility || !!perms?.hotkey_listening)
              }
              label="Accessibility"
              detail={
                accessibilityOk &&
                (!hotkeyNeedsAccessibility || perms?.hotkey_listening)
                  ? "Allowed. Paste and single-key hotkeys work everywhere."
                  : perms?.hotkey_listening
                    ? "The hotkey is live. Enable Accessibility (toggle Fuk off and on after a rebuild) so paste can reach the app you were in."
                    : hotkeyNeedsAccessibility
                    ? "Needed for your single-key hotkey and for pasting at the cursor. If the switch is already on, turn it off and on again after a rebuild. Recheck does not show another system prompt."
                    : "Only needed for pasting at the cursor. Your combo hotkey works without it."
              }
              actionLabel={accessibilityOk ? "Recheck" : "Allow access"}
              onAction={() => void openAccessibility()}
              optional={!hotkeyNeedsAccessibility && !accessibilityOk}
            />
          </Section>
        </Ruled>
      </div>
    </div>
  );
}

function Eyebrow({ children }: { children: ReactNode }) {
  return (
    <p className="flex items-center gap-2 font-mono text-[12px] font-medium uppercase tracking-[0.14em] text-mute">
      <span className="inline-block size-2 bg-red" aria-hidden />
      {children}
    </p>
  );
}

function Ruled({ children }: { children: ReactNode }) {
  return (
    <div className="border border-ink bg-card shadow-press-lg">{children}</div>
  );
}

function Section({ label, children }: { label: string; children: ReactNode }) {
  return (
    <section className="border-b border-ink px-5 py-6 last:border-b-0">
      <h2 className="mb-4 font-mono text-[12px] font-medium uppercase tracking-[0.14em] text-mute">
        {label}
      </h2>
      {children}
    </section>
  );
}

function CardLabel({ children }: { children: ReactNode }) {
  return (
    <h3 className="mb-2.5 mt-6 font-mono text-[11px] font-medium uppercase tracking-[0.12em] text-mute">
      {children}
    </h3>
  );
}

function StatusChip({ tone, label }: { tone: Tone; label: string }) {
  const styles: Record<Tone, string> = {
    ok: "border-ink bg-green text-white",
    warn: "border-ink bg-amber text-white",
    busy: "border-ink bg-card text-ink",
  };
  return (
    <span
      className={`flex shrink-0 items-center gap-1.5 border px-2 py-1 font-mono text-[11px] font-medium uppercase tracking-[0.12em] ${styles[tone]}`}
    >
      {label}
    </span>
  );
}

function Mark({ tone }: { tone: Tone }) {
  const styles: Record<Tone, string> = {
    ok: "bg-green",
    warn: "bg-amber",
    busy: "bg-faint animate-breathe",
  };
  return <span className={`size-2 shrink-0 ${styles[tone]}`} />;
}

function Banner({
  tone,
  children,
}: {
  tone: "error" | "warn";
  children: ReactNode;
}) {
  const cls =
    tone === "error"
      ? "border-ink bg-red text-white"
      : "border-ink bg-amber text-white";
  return (
    <div
      className={`mb-6 border px-3.5 py-2.5 font-sans text-[15px] leading-relaxed ${cls}`}
      role="alert"
    >
      {children}
    </div>
  );
}

function Note({
  className = "",
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      className={`border border-ink bg-paper-deep px-3 py-2.5 font-sans text-[13px] leading-relaxed text-ink-soft ${className}`}
    >
      {children}
    </div>
  );
}

function Keycap({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-[22px] min-w-[22px] items-center justify-center border border-ink bg-card px-1.5 font-mono text-[11px] font-medium leading-none text-ink shadow-keycap">
      {children}
    </kbd>
  );
}

function ProgressBar({ percent }: { percent: number | null }) {
  return (
    <div
      className="h-2 overflow-hidden border border-ink bg-paper"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent ?? undefined}
    >
      <div
        className="h-full bg-red transition-[width] duration-[120ms] ease-in-out"
        style={{ width: percent === null ? "33%" : `${percent}%` }}
      />
    </div>
  );
}

function Btn({
  children,
  onClick,
  variant = "secondary",
  disabled = false,
  className = "",
}: {
  children: ReactNode;
  onClick: () => void;
  variant?: "accent" | "secondary" | "tertiary";
  disabled?: boolean;
  className?: string;
}) {
  const base =
    "focus-ring press-hover inline-flex h-11 shrink-0 items-center justify-center px-5 font-mono text-[13px] font-medium uppercase tracking-[0.08em] disabled:cursor-not-allowed disabled:opacity-45";
  const look = {
    accent: "border border-ink bg-red text-white shadow-press",
    secondary: "border border-ink bg-card text-ink shadow-press",
    tertiary: "border border-line bg-transparent text-ink hover:border-ink",
  }[variant];
  return (
    <button
      type="button"
      className={`${base} ${look} ${className}`}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

function StatusRow({
  ok,
  label,
  detail,
  actionLabel,
  onAction,
  optional = false,
}: {
  ok: boolean;
  label: string;
  detail: string;
  actionLabel: string;
  onAction: () => void;
  optional?: boolean;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <Mark tone={ok ? "ok" : optional ? "busy" : "warn"} />
          <span className="font-display text-[16px] font-semibold tracking-[-0.01em] text-ink">
            {label}
          </span>
          {optional ? (
            <span className="border border-line px-1.5 py-px font-mono text-[10px] uppercase tracking-[0.12em] text-faint">
              Optional
            </span>
          ) : null}
        </div>
        <p className="mt-1 font-sans text-[13px] leading-relaxed text-mute">
          {detail}
        </p>
      </div>
      <Btn variant="tertiary" onClick={onAction}>
        {actionLabel}
      </Btn>
    </div>
  );
}

function Segmented<T extends string>({
  value,
  options,
  ariaLabel,
  onChange,
}: {
  value: T;
  options: { id: T; label: string; disabled?: boolean }[];
  ariaLabel: string;
  onChange: (id: T) => void;
}) {
  return (
    <div
      className="flex border border-ink"
      role="radiogroup"
      aria-label={ariaLabel}
    >
      {options.map((option, i) => {
        const active = option.id === value;
        return (
          <button
            key={option.id}
            type="button"
            role="radio"
            aria-checked={active}
            disabled={option.disabled}
            className={`focus-ring h-11 flex-1 px-3 font-mono text-[13px] font-medium uppercase tracking-[0.08em] transition-colors duration-[120ms] ease-in-out disabled:cursor-not-allowed disabled:opacity-35 ${
              i > 0 ? "border-l border-ink" : ""
            } ${
              active
                ? "bg-ink text-paper"
                : "bg-card text-ink hover:bg-paper-deep"
            }`}
            onClick={() => onChange(option.id)}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

function OptionGroup({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col border border-ink" role="radiogroup" aria-label={label}>
      {children}
    </div>
  );
}

function OptionRow({
  selected,
  label,
  hint,
  disabled = false,
  onSelect,
}: {
  selected: boolean;
  label: string;
  hint: string;
  disabled?: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      className={`focus-ring flex w-full items-start gap-3 border-b border-ink px-3 py-2.5 text-left last:border-b-0 disabled:cursor-not-allowed disabled:opacity-40 ${
        selected ? "bg-paper-deep" : "bg-card hover:bg-paper"
      }`}
    >
      <span
        className={`mt-1.5 size-2 shrink-0 ${
          selected ? "bg-red" : "border border-ink bg-card"
        }`}
      />
      <span className="min-w-0">
        <span className="block font-display text-[16px] font-semibold tracking-[-0.01em] text-ink">
          {label}
        </span>
        <span className="mt-0.5 block font-sans text-[13px] leading-relaxed text-mute">
          {hint}
        </span>
      </span>
    </button>
  );
}

function Select({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
}) {
  return (
    <div className="relative">
      <select
        aria-label={label}
        className="focus-ring h-11 w-full appearance-none border border-ink bg-card px-3 pr-9 font-sans text-[15px] text-ink"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <svg
        viewBox="0 0 24 24"
        className="pointer-events-none absolute right-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-ink"
        aria-hidden
      >
        <path
          d="M7 10l5 5 5-5"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
        />
      </svg>
    </div>
  );
}
