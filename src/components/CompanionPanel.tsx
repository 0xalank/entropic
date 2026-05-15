import {
  Check,
  GripHorizontal,
  Link2,
  Lock,
  MousePointer2,
  Pin,
  PinOff,
  RefreshCw,
  Sparkles,
  X,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  useEffect,
  useCallback,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { Chat, type ChatSession, type ChatSessionActionRequest } from "../pages/Chat";
import type { Page } from "./Layout";
import type { VoiceSpeechVoice } from "../desktop/voice/voicePreferences";
import {
  buildCompanionContextPrompt,
  formatCompanionFocusLabel,
  getCompanionState,
  runCompanionTool,
  setCompanionSkillGrant,
  type CompanionState,
  type CompanionToolRunResult,
} from "../lib/companion";

type CompanionPanelProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  gatewayRunning: boolean;
  gatewayStarting: boolean;
  gatewayRetryIn: number | null;
  gatewayLifecycleLabel?: string | null;
  onGatewayConnectionReady?: () => void;
  onStartGateway: () => void;
  onRecoverProxyAuth?: () => Promise<boolean> | boolean;
  useLocalKeys: boolean;
  selectedModel: string;
  onModelChange: (model: string) => void;
  imageModel: string;
  imageGenerationModel: string;
  textToSpeechModel: string;
  audioUnderstandingModel: string;
  voiceSpeechRate: number;
  voiceSpeechVoice: VoiceSpeechVoice;
  integrationsSyncing?: boolean;
  integrationsMissing?: boolean;
  onNavigate: (page: Page) => void;
  onSessionsChange: (sessions: ChatSession[], currentKey: string | null) => void;
  requestedSession?: string | null;
  requestedSessionAction?: ChatSessionActionRequest | null;
  nativeWindow?: boolean;
};

type PanelPoint = { x: number; y: number };
type PanelDragState = { sx: number; sy: number; ox: number; oy: number };

const DEFAULT_PANEL_SIZE = { w: 440, h: 680 };

function clampPanelPosition(next: PanelPoint, size = DEFAULT_PANEL_SIZE): PanelPoint {
  if (typeof window === "undefined") return next;
  const maxX = Math.max(8, window.innerWidth - size.w - 8);
  const maxY = Math.max(8, window.innerHeight - 96);
  return {
    x: Math.min(Math.max(8, next.x), maxX),
    y: Math.min(Math.max(8, next.y), maxY),
  };
}

function initialPanelPosition(): PanelPoint {
  if (typeof window === "undefined") {
    return { x: 32, y: 48 };
  }
  return clampPanelPosition({
    x: window.innerWidth - DEFAULT_PANEL_SIZE.w - 28,
    y: 52,
  });
}

function statusLabel(status?: string | null): string {
  switch (status) {
    case "ready":
      return "Ready";
    case "needs_grant":
      return "Needs Permission";
    case "waiting_for_bridge":
      return "Waiting For Bridge";
    default:
      return status ? status.replace(/_/g, " ") : "Idle";
  }
}

export function CompanionPanel({
  open,
  onOpenChange,
  gatewayRunning,
  gatewayStarting,
  gatewayRetryIn,
  gatewayLifecycleLabel,
  onGatewayConnectionReady,
  onStartGateway,
  onRecoverProxyAuth,
  useLocalKeys,
  selectedModel,
  onModelChange,
  imageModel,
  imageGenerationModel,
  textToSpeechModel,
  audioUnderstandingModel,
  voiceSpeechRate,
  voiceSpeechVoice,
  integrationsSyncing,
  integrationsMissing,
  onNavigate,
  onSessionsChange,
  requestedSession,
  requestedSessionAction,
  nativeWindow = false,
}: CompanionPanelProps) {
  const [state, setState] = useState<CompanionState | null>(null);
  const [sticky, setSticky] = useState(false);
  const [attachToFocusedApp, setAttachToFocusedApp] = useState(true);
  const [position, setPosition] = useState<PanelPoint>(() => initialPanelPosition());
  const [loadingState, setLoadingState] = useState(false);
  const [granting, setGranting] = useState(false);
  const [toolRunning, setToolRunning] = useState(false);
  const [toolResult, setToolResult] = useState<CompanionToolRunResult | null>(null);
  const [toolError, setToolError] = useState<string | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<PanelDragState | null>(null);

  async function refreshState() {
    setLoadingState(true);
    try {
      const next = await getCompanionState();
      setState(next);
    } catch (error) {
      console.warn("[Entropic] Failed to refresh Companion state:", error);
    } finally {
      setLoadingState(false);
    }
  }

  useEffect(() => {
    void refreshState();
    let disposed = false;
    const disposers: Array<() => void> = [];
    Promise.all([
      listen<CompanionState>("companion-state-changed", (event) => {
        setState(event.payload);
      }),
      listen("focus-changed", () => {
        void refreshState();
      }),
      listen("companion-toggle-requested", () => {
        if (!nativeWindow) {
          onOpenChange(!open);
        }
        void refreshState();
      }),
    ]).then((unlisten) => {
      if (disposed) {
        unlisten.forEach((dispose) => dispose());
        return;
      }
      disposers.push(...unlisten);
    });
    return () => {
      disposed = true;
      disposers.forEach((dispose) => dispose());
    };
  }, [nativeWindow, onOpenChange, open]);

  useEffect(() => {
    if (!open) return;
    void refreshState();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    if (nativeWindow) {
      const onKeyDown = (event: KeyboardEvent) => {
        if (event.key === "Escape") {
          onOpenChange(false);
        }
      };
      window.addEventListener("keydown", onKeyDown);
      return () => {
        window.removeEventListener("keydown", onKeyDown);
      };
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !sticky) {
        onOpenChange(false);
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (sticky) return;
      const target = event.target as HTMLElement | null;
      if (!target) return;
      if (panelRef.current?.contains(target) || target.closest("[data-companion-trigger]")) {
        return;
      }
      onOpenChange(false);
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [nativeWindow, open, sticky, onOpenChange]);

  useEffect(() => {
    if (nativeWindow || !attachToFocusedApp || !open) return;
    setPosition(clampPanelPosition(initialPanelPosition()));
  }, [nativeWindow, attachToFocusedApp, open, state?.hostFocus?.bundleId, state?.hostFocus?.windowTitle]);

  function startDrag(event: ReactMouseEvent<HTMLDivElement>) {
    const target = event.target as HTMLElement;
    if (target.closest("button, input, textarea, select, [role='button']")) return;
    event.preventDefault();
    if (nativeWindow) {
      void getCurrentWindow().startDragging();
      return;
    }
    setAttachToFocusedApp(false);
    dragRef.current = {
      sx: event.clientX,
      sy: event.clientY,
      ox: position.x,
      oy: position.y,
    };

    function onMove(moveEvent: MouseEvent) {
      if (!dragRef.current) return;
      setPosition(
        clampPanelPosition({
          x: dragRef.current.ox + moveEvent.clientX - dragRef.current.sx,
          y: dragRef.current.oy + moveEvent.clientY - dragRef.current.sy,
        }),
      );
    }

    function onUp() {
      dragRef.current = null;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    }

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  async function enablePrimarySkill() {
    if (!state?.primarySkill || granting) return;
    setGranting(true);
    try {
      const next = await setCompanionSkillGrant(state.primarySkill.id, true);
      setState(next);
      setToolError(null);
    } catch (error) {
      setToolError(error instanceof Error ? error.message : String(error));
    } finally {
      setGranting(false);
    }
  }

  async function inspectSelection() {
    const skill = state?.primarySkill;
    if (!skill || toolRunning) return;
    setToolRunning(true);
    setToolError(null);
    try {
      const result = await runCompanionTool(skill.id, "get_selection");
      setToolResult(result);
    } catch (error) {
      setToolError(error instanceof Error ? error.message : String(error));
    } finally {
      setToolRunning(false);
    }
  }

  async function createFrame() {
    const skill = state?.primarySkill;
    if (!skill || toolRunning) return;
    setToolRunning(true);
    setToolError(null);
    try {
      const result = await runCompanionTool(skill.id, "create_frame", {
        name: "Entropic Companion Frame",
        width: 720,
        height: 480,
      });
      setToolResult(result);
      window.setTimeout(() => {
        void refreshState();
      }, 800);
    } catch (error) {
      setToolError(error instanceof Error ? error.message : String(error));
    } finally {
      setToolRunning(false);
    }
  }

  const hostFocus = state?.hostFocus || state?.focus || null;
  const primarySkill = state?.primarySkill || null;
  const canCreateFrame = primarySkill?.tools.some((tool) => tool.id === "create_frame") ?? false;
  const companionContext = useMemo(
    () => buildCompanionContextPrompt(state, toolResult?.output),
    [state, toolResult],
  );
  const resolveCompanionContext = useCallback(async () => {
    if (toolRunning) {
      return companionContext;
    }

    const nextState = await getCompanionState();
    setState(nextState);
    const skill = nextState.primarySkill;
    const canInspect =
      skill?.granted &&
      skill.status === "ready" &&
      skill.tools.some((tool) => tool.id === "get_selection");

    if (!skill || !canInspect) {
      return buildCompanionContextPrompt(nextState);
    }

    setToolRunning(true);
    setToolError(null);
    try {
      const result = await runCompanionTool(skill.id, "get_selection");
      setToolResult(result);
      return buildCompanionContextPrompt(nextState, result.output);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setToolError(message);
      return [
        buildCompanionContextPrompt(nextState),
        `Companion selection probe failed before this turn: ${message}`,
      ]
        .filter(Boolean)
        .join("\n");
    } finally {
      setToolRunning(false);
    }
  }, [companionContext, toolRunning]);

  if (!open) {
    return null;
  }

  const panelStyle: CSSProperties = nativeWindow
    ? {
        inset: 0,
        borderColor: "var(--border-subtle)",
        boxShadow: "none",
      }
    : {
        left: position.x,
        top: position.y,
        width: DEFAULT_PANEL_SIZE.w,
        height: `min(${DEFAULT_PANEL_SIZE.h}px, calc(100vh - 76px))`,
        borderColor: "var(--border-subtle)",
        resize: "both",
        boxShadow: "0 28px 80px rgba(0,0,0,0.32), 0 0 0 0.5px var(--border-subtle)",
      };

  return (
    <div
      ref={panelRef}
      className={
        nativeWindow
          ? "fixed z-10 flex min-h-0 min-w-0 flex-col overflow-hidden border bg-[var(--bg-card)] text-[var(--text-primary)]"
          : "fixed z-[80] flex min-h-[520px] min-w-[360px] flex-col overflow-hidden rounded-xl border bg-[var(--bg-card)] text-[var(--text-primary)] shadow-2xl"
      }
      style={panelStyle}
      onClick={(event) => event.stopPropagation()}
    >
      <div
        className="flex cursor-grab select-none items-center gap-2 border-b px-3 py-2 active:cursor-grabbing"
        style={{
          borderColor: "var(--border-subtle)",
          background: "var(--bg-secondary)",
        }}
        onMouseDown={startDrag}
      >
        <GripHorizontal className="h-4 w-4 shrink-0 text-[var(--text-tertiary)]" />
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-[var(--purple-accent)] text-white">
          <Sparkles className="h-4 w-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-semibold">Companion</div>
          <div className="truncate text-[11px] text-[var(--text-secondary)]">
            {formatCompanionFocusLabel(hostFocus)}
            {hostFocus?.windowTitle ? ` · ${hostFocus.windowTitle}` : ""}
          </div>
        </div>
        <button
          type="button"
          onClick={() => setAttachToFocusedApp((current) => !current)}
          className="flex h-8 w-8 items-center justify-center rounded-lg border transition-colors hover:bg-[var(--border-subtle)]"
          style={{ borderColor: "var(--border-subtle)" }}
          title={attachToFocusedApp ? "Detach from focused app" : "Attach to focused app"}
          aria-label={attachToFocusedApp ? "Detach from focused app" : "Attach to focused app"}
        >
          <Link2 className="h-4 w-4" />
        </button>
        <button
          type="button"
          onClick={() => setSticky((current) => !current)}
          className="flex h-8 w-8 items-center justify-center rounded-lg border transition-colors hover:bg-[var(--border-subtle)]"
          style={{ borderColor: "var(--border-subtle)" }}
          title={sticky ? "Unpin Companion" : "Keep Companion visible"}
          aria-label={sticky ? "Unpin Companion" : "Keep Companion visible"}
        >
          {sticky ? <Pin className="h-4 w-4" /> : <PinOff className="h-4 w-4" />}
        </button>
        <button
          type="button"
          onClick={() => onOpenChange(false)}
          className="flex h-8 w-8 items-center justify-center rounded-lg border transition-colors hover:bg-[var(--border-subtle)]"
          style={{ borderColor: "var(--border-subtle)" }}
          title="Close Companion"
          aria-label="Close Companion"
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      <div
        className="border-b px-3 py-2"
        style={{ borderColor: "var(--border-subtle)", background: "var(--bg-card)" }}
      >
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <span className="inline-flex items-center gap-1 rounded-md border px-2 py-1 text-[11px] font-medium"
                style={{ borderColor: "var(--border-subtle)", background: "var(--bg-secondary)" }}
              >
                <MousePointer2 className="h-3.5 w-3.5" />
                {primarySkill ? primarySkill.name : "No Skill"}
              </span>
              <span className="inline-flex rounded-md border px-2 py-1 text-[11px]"
                style={{ borderColor: "var(--border-subtle)", color: "var(--text-secondary)" }}
              >
                {statusLabel(primarySkill?.status)}
              </span>
            </div>
            <div className="mt-1 truncate text-[11px] text-[var(--text-secondary)]">
              {primarySkill
                ? primarySkill.unitOfWork.join(" · ")
                : "Focus a supported app to attach a first-party skill."}
            </div>
          </div>
          <button
            type="button"
            onClick={() => void refreshState()}
            disabled={loadingState}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border transition-colors hover:bg-[var(--border-subtle)] disabled:opacity-50"
            style={{ borderColor: "var(--border-subtle)" }}
            title="Refresh Companion state"
            aria-label="Refresh Companion state"
          >
            <RefreshCw className={`h-4 w-4 ${loadingState ? "animate-spin" : ""}`} />
          </button>
        </div>

        {primarySkill && !primarySkill.granted ? (
          <div className="mt-2 flex items-center justify-between gap-3 rounded-lg border px-2.5 py-2 text-[11px]"
            style={{ borderColor: "var(--border-subtle)", background: "var(--bg-secondary)" }}
          >
            <div className="min-w-0">
              <div className="flex items-center gap-1.5 font-medium">
                <Lock className="h-3.5 w-3.5" />
                Permission required
              </div>
              <div className="mt-0.5 text-[var(--text-secondary)]">
                Enable before this skill reads or writes host app content.
              </div>
            </div>
            <button
              type="button"
              onClick={() => void enablePrimarySkill()}
              disabled={granting}
              className="inline-flex shrink-0 items-center gap-1 rounded-md bg-[var(--text-primary)] px-2.5 py-1.5 font-medium text-[var(--bg-card)] disabled:opacity-50"
            >
              <Check className="h-3.5 w-3.5" />
              Enable
            </button>
          </div>
        ) : null}

        {primarySkill?.granted ? (
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void inspectSelection()}
              disabled={toolRunning}
              className="inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1.5 text-[11px] font-medium transition-colors hover:bg-[var(--border-subtle)] disabled:opacity-50"
              style={{ borderColor: "var(--border-subtle)" }}
            >
              <MousePointer2 className="h-3.5 w-3.5" />
              {toolRunning ? "Inspecting" : "Inspect Selection"}
            </button>
            {canCreateFrame ? (
              <button
                type="button"
                onClick={() => void createFrame()}
                disabled={toolRunning}
                className="inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1.5 text-[11px] font-medium transition-colors hover:bg-[var(--border-subtle)] disabled:opacity-50"
                style={{ borderColor: "var(--border-subtle)" }}
              >
                <Sparkles className="h-3.5 w-3.5" />
                Create Frame
              </button>
            ) : null}
            {toolResult ? (
              <span className="truncate text-[11px] text-[var(--text-secondary)]">
                {toolResult.summary}
              </span>
            ) : null}
          </div>
        ) : null}

        {toolError ? (
          <div className="mt-2 rounded-lg border border-red-500/20 bg-red-500/10 px-2.5 py-2 text-[11px] text-red-600">
            {toolError}
          </div>
        ) : null}

        {toolResult ? (
          <pre className="mt-2 max-h-28 overflow-auto rounded-lg border p-2 text-[10px] leading-relaxed text-[var(--text-secondary)]"
            style={{ borderColor: "var(--border-subtle)", background: "var(--bg-secondary)" }}
          >
            {JSON.stringify(toolResult.output, null, 2)}
          </pre>
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-hidden">
        <Chat
          isVisible={open}
          gatewayRunning={gatewayRunning}
          gatewayStarting={gatewayStarting}
          gatewayRetryIn={gatewayRetryIn}
          gatewayLifecycleLabel={gatewayLifecycleLabel ?? null}
          onGatewayConnectionReady={onGatewayConnectionReady}
          onStartGateway={onStartGateway}
          onRecoverProxyAuth={onRecoverProxyAuth}
          useLocalKeys={useLocalKeys}
          selectedModel={selectedModel}
          onModelChange={onModelChange}
          imageModel={imageModel}
          imageGenerationModel={imageGenerationModel}
          textToSpeechModel={textToSpeechModel}
          audioUnderstandingModel={audioUnderstandingModel}
          voiceSpeechRate={voiceSpeechRate}
          voiceSpeechVoice={voiceSpeechVoice}
          integrationsSyncing={integrationsSyncing}
          integrationsMissing={integrationsMissing}
          onNavigate={onNavigate}
          onSessionsChange={onSessionsChange}
          requestedSession={requestedSession}
          requestedSessionAction={requestedSessionAction}
          wideLayout
          companionContext={companionContext}
          getCompanionContext={resolveCompanionContext}
        />
      </div>
    </div>
  );
}
