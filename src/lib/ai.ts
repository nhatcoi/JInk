import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { languageName } from "./languages";
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
  images: string[] = [],
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
      search_key: settings.searchKey,
    },
    messages,
    images,
    requestId,
  }).catch((err) => {
    handlers.onError(String(err));
    cleanup();
  });

  return cleanup;
}

/** Turn a raw reqwest/network error into something a user can act on. */
export function friendlyAiError(raw: string): string {
  if (/error sending request|tcp connect|connection refused|dns error/i.test(raw)) {
    return "Can't reach the AI server — check Base URL/Model in Settings, or start your local AI.";
  }
  return raw;
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

export function explainPrompt(text: string, lang: string): ChatMessage[] {
  return [
    {
      role: "system",
      content: `You are a concise explainer. Explain the meaning of the word, sentence, or attached image the user sends. Answer in ${languageName(lang)} if you are fluent in it; otherwise answer in English. Be brief: 1-3 short sentences, no preamble, no quotes. If it's a term or fact needing current info, use web search when available.`,
    },
    { role: "user", content: text || "Explain the attached image." },
  ];
}

export function translatePrompt(
  text: string,
  from: string,
  to: string,
): ChatMessage[] {
  const instruction =
    from === "auto"
      ? `Detect the source language and translate it to ${languageName(to)}.`
      : `Translate from ${languageName(from)} to ${languageName(to)}.`;
  return [
    {
      role: "system",
      content: `${instruction} Return ONLY the translation, no quotes, no explanation.`,
    },
    { role: "user", content: text },
  ];
}
