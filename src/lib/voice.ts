import type { Settings } from "./settings";

/** Record mic audio until stop() is called, then transcribe via an
 *  OpenAI-compatible /audio/transcriptions endpoint. */
export async function startRecording(): Promise<{
  stop: () => Promise<Blob>;
}> {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  const chunks: BlobPart[] = [];
  const rec = new MediaRecorder(stream);
  rec.ondataavailable = (e) => e.data.size > 0 && chunks.push(e.data);
  rec.start();

  return {
    stop: () =>
      new Promise<Blob>((resolve) => {
        rec.onstop = () => {
          stream.getTracks().forEach((t) => t.stop());
          resolve(new Blob(chunks, { type: "audio/webm" }));
        };
        rec.stop();
      }),
  };
}

export async function transcribe(
  blob: Blob,
  settings: Settings,
): Promise<string> {
  const form = new FormData();
  form.append("file", blob, "audio.webm");
  form.append("model", "whisper-1");

  const res = await fetch(
    `${settings.aiBaseUrl.replace(/\/$/, "")}/audio/transcriptions`,
    {
      method: "POST",
      headers: { Authorization: `Bearer ${settings.aiKey}` },
      body: form,
    },
  );
  if (!res.ok) throw new Error(`STT ${res.status}: ${await res.text()}`);
  const json = await res.json();
  return json.text ?? "";
}
