import type { Settings } from "./settings";

const CANDIDATE_MIME_TYPES = [
  "audio/webm;codecs=opus",
  "audio/webm",
  "audio/mp4",
  "audio/ogg;codecs=opus",
  "audio/ogg",
];

function pickMimeType(): string | undefined {
  return CANDIDATE_MIME_TYPES.find((t) => MediaRecorder.isTypeSupported(t));
}


export async function startRecording(deviceId?: string): Promise<{
  stream: MediaStream;
  stop: () => Promise<Blob>;
}> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: deviceId ? { deviceId: { exact: deviceId } } : true,
  });
  const mimeType = pickMimeType();
  const chunks: BlobPart[] = [];
  const rec = new MediaRecorder(stream, mimeType ? { mimeType } : undefined);
  rec.ondataavailable = (e) => e.data.size > 0 && chunks.push(e.data);

  rec.start(250);

  return {
    stream,
    stop: () =>
      new Promise<Blob>((resolve, reject) => {
        rec.onstop = () => {
          stream.getTracks().forEach((t) => t.stop());
          const blob = new Blob(chunks, { type: mimeType ?? "audio/webm" });
          if (blob.size === 0) {
            reject(new Error("Recording captured no audio data"));
            return;
          }
          resolve(blob);
        };
        rec.stop();
      }),
  };
}


export async function listMicDevices(): Promise<MediaDeviceInfo[]> {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  stream.getTracks().forEach((t) => t.stop());
  const devices = await navigator.mediaDevices.enumerateDevices();
  return devices.filter((d) => d.kind === "audioinput");
}

const EXT_BY_MIME: Record<string, string> = {
  "audio/webm": "webm",
  "audio/mp4": "mp4",
  "audio/ogg": "ogg",
};

export async function transcribe(
  blob: Blob,
  settings: Settings,
): Promise<string> {
  const baseMime = blob.type.split(";")[0];
  const ext = EXT_BY_MIME[baseMime] ?? "webm";
  const form = new FormData();
  form.append("file", blob, `audio.${ext}`);
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
