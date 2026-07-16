import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Settings } from "./settings";

export type ChatMessage = { role: "system" | "user" | "assistant"; content: string };

type StreamHandlers = {
  onToken: (t: string) => void;
  onDone: () => void;
  onError: (e: string) => void;
};

/** Kick off a streaming completion; returns a cancel fn that detaches listeners. */
export async function runAiStream(
  settings: Settings,
  messages: ChatMessage[],
  handlers: StreamHandlers,
): Promise<() => void> {
  const requestId = crypto.randomUUID();
  const unlisten: UnlistenFn[] = [];

  unlisten.push(
    await listen<[string, string]>("ai-token", (e) => {
      if (e.payload[0] === requestId) handlers.onToken(e.payload[1]);
    }),
  );
  unlisten.push(
    await listen<string>("ai-done", (e) => {
      if (e.payload === requestId) {
        handlers.onDone();
        cleanup();
      }
    }),
  );
  unlisten.push(
    await listen<[string, string]>("ai-error", (e) => {
      if (e.payload[0] === requestId) {
        handlers.onError(e.payload[1]);
        cleanup();
      }
    }),
  );

  function cleanup() {
    unlisten.forEach((u) => u());
  }

  invoke("ai_stream", {
    config: {
      base_url: settings.aiBaseUrl,
      model: settings.aiModel,
      api_key: settings.aiKey,
    },
    messages,
    requestId,
  }).catch((err) => {
    handlers.onError(String(err));
    cleanup();
  });

  return cleanup;
}

export function enhancePrompt(text: string): ChatMessage[] {
  return [
    {
      role: "system",
      content:
        "You are a writing assistant. Improve the user's text: fix grammar, spelling and clarity while preserving meaning, tone and language. Return ONLY the improved text, no quotes, no explanation.",
    },
    { role: "user", content: text },
  ];
}

export function translatePrompt(
  text: string,
  from: string,
  to: string,
): ChatMessage[] {
  const lang = (c: string) => (c === "vi" ? "Vietnamese" : "English");
  return [
    {
      role: "system",
      content: `Translate from ${lang(from)} to ${lang(
        to,
      )}. Return ONLY the translation, no quotes, no explanation.`,
    },
    { role: "user", content: text },
  ];
}
