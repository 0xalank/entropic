import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CompanionPanel } from "./CompanionPanel";
import type { Page } from "./Layout";
import { useAuth } from "../contexts/AuthContext";
import { defaultUseLocalKeys } from "../lib/buildProfile";
import { getGatewayStatusCached } from "../lib/gateway-status";
import {
  primeDesktopSettings,
  updateDesktopSettings,
  type DesktopSettingsSnapshot,
} from "../lib/settingsStore";
import { type ChatSession, type ChatSessionActionRequest } from "../pages/Chat";
import {
  DEFAULT_VOICE_SPEECH_RATE,
  DEFAULT_VOICE_SPEECH_VOICE,
  normalizeVoiceSpeechRate,
  normalizeVoiceSpeechVoice,
  type VoiceSpeechVoice,
} from "../desktop/voice/voicePreferences";

type GatewayLaunchMode = "stopped" | "local" | "proxy";

type AppBootstrapState = {
  settings: DesktopSettingsSnapshot;
  gatewayLaunchMode: GatewayLaunchMode;
  gatewayContainerRunning: boolean;
  gatewayHealthStatus: string;
};

const DEFAULT_PROXY_MODEL = "openai/gpt-5.5";
const DEFAULT_LOCAL_MODEL = "anthropic/claude-opus-4-6:thinking";
const DEFAULT_IMAGE_MODEL = "google/gemini-3.1-flash-image-preview";
const DEFAULT_PROXY_IMAGE_GENERATION_MODEL = "google/gemini-3.1-flash-image-preview";
const DEFAULT_LOCAL_IMAGE_GENERATION_MODEL = "google/gemini-3.1-flash-image-preview";
const DEFAULT_PROXY_AUDIO_UNDERSTANDING_MODEL = "venice/nvidia/parakeet-tdt-0.6b-v3";
const DEFAULT_LOCAL_AUDIO_UNDERSTANDING_MODEL = "google/gemini-3-flash-preview";
const DEFAULT_PROXY_TEXT_TO_SPEECH_MODEL = "venice/tts-kokoro";
const DEFAULT_LOCAL_TEXT_TO_SPEECH_MODEL = "openai/gpt-4o-mini-tts";

function isGatewayHealthyStatus(status: string, gatewayContainerRunning: boolean) {
  if (!gatewayContainerRunning) return false;
  return status.trim().toLowerCase() === "healthy";
}

function valueOrDefault(value: string | undefined, fallback: string) {
  return value && value.trim() ? value : fallback;
}

export function CompanionWindowApp() {
  const { isAuthConfigured } = useAuth();
  const [loading, setLoading] = useState(true);
  const [starting, setStarting] = useState(false);
  const [gatewayRunning, setGatewayRunning] = useState(false);
  const [gatewayContainerRunning, setGatewayContainerRunning] = useState(false);
  const [gatewayLaunchMode, setGatewayLaunchMode] = useState<GatewayLaunchMode>("stopped");
  const [gatewayHealthStatus, setGatewayHealthStatus] = useState("stopped");
  const [gatewayError, setGatewayError] = useState<string | null>(null);
  const [useLocalKeys, setUseLocalKeys] = useState(defaultUseLocalKeys);
  const [selectedModel, setSelectedModel] = useState(
    defaultUseLocalKeys ? DEFAULT_LOCAL_MODEL : DEFAULT_PROXY_MODEL,
  );
  const [imageModel, setImageModel] = useState(DEFAULT_IMAGE_MODEL);
  const [imageGenerationModel, setImageGenerationModel] = useState(
    defaultUseLocalKeys
      ? DEFAULT_LOCAL_IMAGE_GENERATION_MODEL
      : DEFAULT_PROXY_IMAGE_GENERATION_MODEL,
  );
  const [textToSpeechModel, setTextToSpeechModel] = useState(
    defaultUseLocalKeys ? DEFAULT_LOCAL_TEXT_TO_SPEECH_MODEL : DEFAULT_PROXY_TEXT_TO_SPEECH_MODEL,
  );
  const [audioUnderstandingModel, setAudioUnderstandingModel] = useState(
    defaultUseLocalKeys
      ? DEFAULT_LOCAL_AUDIO_UNDERSTANDING_MODEL
      : DEFAULT_PROXY_AUDIO_UNDERSTANDING_MODEL,
  );
  const [voiceSpeechRate, setVoiceSpeechRate] = useState(DEFAULT_VOICE_SPEECH_RATE);
  const [voiceSpeechVoice, setVoiceSpeechVoice] =
    useState<VoiceSpeechVoice>(DEFAULT_VOICE_SPEECH_VOICE);
  const [currentChatSession, setCurrentChatSession] = useState<string | null>(null);
  const [pendingChatAction] = useState<ChatSessionActionRequest | null>(null);

  async function refreshBootstrap() {
    const bootstrap = await invoke<AppBootstrapState>("get_app_bootstrap_state");
    const running = await getGatewayStatusCached({ force: true });
    const healthy = isGatewayHealthyStatus(
      bootstrap.gatewayHealthStatus,
      bootstrap.gatewayContainerRunning,
    );

    setGatewayRunning(running || healthy);
    setGatewayContainerRunning(bootstrap.gatewayContainerRunning);
    setGatewayLaunchMode(bootstrap.gatewayLaunchMode);
    setGatewayHealthStatus(bootstrap.gatewayHealthStatus);
    return bootstrap;
  }

  useEffect(() => {
    let cancelled = false;

    async function load() {
      try {
        const bootstrap = await refreshBootstrap();
        if (cancelled) return;

        primeDesktopSettings(bootstrap.settings);
        const isLocal =
          typeof bootstrap.settings.useLocalKeys === "boolean"
            ? bootstrap.settings.useLocalKeys
            : defaultUseLocalKeys;
        setUseLocalKeys(isLocal);
        setSelectedModel(
          valueOrDefault(bootstrap.settings.selectedModel, isLocal ? DEFAULT_LOCAL_MODEL : DEFAULT_PROXY_MODEL),
        );
        setImageModel(valueOrDefault(bootstrap.settings.imageModel, DEFAULT_IMAGE_MODEL));
        setImageGenerationModel(
          valueOrDefault(
            bootstrap.settings.imageGenerationModel,
            isLocal ? DEFAULT_LOCAL_IMAGE_GENERATION_MODEL : DEFAULT_PROXY_IMAGE_GENERATION_MODEL,
          ),
        );
        setTextToSpeechModel(
          valueOrDefault(
            bootstrap.settings.textToSpeechModel,
            isLocal ? DEFAULT_LOCAL_TEXT_TO_SPEECH_MODEL : DEFAULT_PROXY_TEXT_TO_SPEECH_MODEL,
          ),
        );
        setAudioUnderstandingModel(
          valueOrDefault(
            bootstrap.settings.audioUnderstandingModel,
            isLocal ? DEFAULT_LOCAL_AUDIO_UNDERSTANDING_MODEL : DEFAULT_PROXY_AUDIO_UNDERSTANDING_MODEL,
          ),
        );
        setVoiceSpeechRate(
          normalizeVoiceSpeechRate(bootstrap.settings.voiceSpeechRate ?? DEFAULT_VOICE_SPEECH_RATE),
        );
        setVoiceSpeechVoice(
          normalizeVoiceSpeechVoice(
            bootstrap.settings.voiceSpeechVoice ?? DEFAULT_VOICE_SPEECH_VOICE,
          ),
        );
        setGatewayError(null);
      } catch (error) {
        if (!cancelled) {
          setGatewayError(error instanceof Error ? error.message : String(error));
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    void load();
    const interval = window.setInterval(() => {
      void refreshBootstrap().catch(() => undefined);
    }, 5000);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, []);

  async function hideWindow() {
    try {
      await invoke("hide_companion_window");
    } catch {
      await getCurrentWindow().hide();
    }
  }

  async function handleStartGateway() {
    if (gatewayRunning || starting) return;
    setGatewayError(null);
    setStarting(true);
    try {
      if (useLocalKeys) {
        setGatewayContainerRunning(true);
        setGatewayHealthStatus("starting");
        setGatewayLaunchMode("local");
        await invoke("start_gateway", { model: selectedModel });
        window.setTimeout(() => {
          void refreshBootstrap().catch(() => undefined);
        }, 2000);
        return;
      }

      if (!isAuthConfigured) {
        await invoke("show_main_window").catch(() => undefined);
      }
      await emit("companion-start-gateway-requested", { requestedAt: Date.now() });
      window.setTimeout(() => {
        void refreshBootstrap().catch(() => undefined);
      }, 2500);
    } catch (error) {
      setGatewayError(error instanceof Error ? error.message : String(error));
    } finally {
      window.setTimeout(() => setStarting(false), 4000);
    }
  }

  async function handleModelChange(model: string) {
    setSelectedModel(model);
    try {
      await updateDesktopSettings({ selectedModel: model });
    } catch (error) {
      console.warn("[Entropic] Failed to save Companion model selection:", error);
    }
  }

  function handleSessionsChange(_sessions: ChatSession[], currentKey: string | null) {
    setCurrentChatSession((current) => currentKey ?? current);
  }

  function handleNavigate(page: Page) {
    void invoke("show_main_window").catch(() => undefined);
    void emit("companion-open-page-requested", page);
  }

  const gatewayStarting =
    loading ||
    starting ||
    (!gatewayRunning && gatewayContainerRunning && gatewayLaunchMode !== "stopped");
  const gatewayLifecycleLabel = useMemo(() => {
    if (gatewayError) return gatewayError;
    if (loading) return "Loading Companion";
    if (starting) return "Starting secure sandbox";
    if (!gatewayRunning && gatewayContainerRunning && gatewayHealthStatus === "starting") {
      return "Verifying sandbox health";
    }
    if (!gatewayRunning && gatewayContainerRunning) {
      return "Connecting to your assistant";
    }
    return null;
  }, [gatewayContainerRunning, gatewayError, gatewayHealthStatus, gatewayRunning, loading, starting]);

  return (
    <div className="h-screen w-screen overflow-hidden bg-[var(--bg-card)]">
      <CompanionPanel
        open
        onOpenChange={(next) => {
          if (!next) void hideWindow();
        }}
        gatewayRunning={gatewayRunning}
        gatewayStarting={gatewayStarting}
        gatewayRetryIn={null}
        gatewayLifecycleLabel={gatewayLifecycleLabel}
        onGatewayConnectionReady={() => {
          setGatewayRunning(true);
          setStarting(false);
        }}
        onStartGateway={handleStartGateway}
        onRecoverProxyAuth={async () => {
          await invoke("show_main_window").catch(() => undefined);
          await emit("companion-start-gateway-requested", { requestedAt: Date.now() });
          return false;
        }}
        useLocalKeys={useLocalKeys}
        selectedModel={selectedModel}
        onModelChange={handleModelChange}
        imageModel={imageModel}
        imageGenerationModel={imageGenerationModel}
        textToSpeechModel={textToSpeechModel}
        audioUnderstandingModel={audioUnderstandingModel}
        voiceSpeechRate={voiceSpeechRate}
        voiceSpeechVoice={voiceSpeechVoice}
        integrationsSyncing={false}
        integrationsMissing={false}
        onNavigate={handleNavigate}
        onSessionsChange={handleSessionsChange}
        requestedSession={currentChatSession}
        requestedSessionAction={pendingChatAction}
        nativeWindow
      />
    </div>
  );
}
