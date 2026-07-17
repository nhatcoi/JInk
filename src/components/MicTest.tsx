import { useEffect, useRef, useState } from "react";
import { Mic, Play, RefreshCw, Square } from "lucide-react";
import { listMicDevices, startRecording } from "@/lib/voice";
import { cn } from "@/lib/utils";
import { Combobox } from "@/components/Combobox";

type Recorder = Awaited<ReturnType<typeof startRecording>>;

type Props = {
  deviceId: string;
  onDeviceChange: (id: string) => void;
};

/** Mic picker + record/playback test — confirms the selected input actually
 *  captures audio before the user relies on it mid-task. */
export function MicTest({ deviceId, onDeviceChange }: Props) {
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
  const [loadingDevices, setLoadingDevices] = useState(false);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [level, setLevel] = useState(0);
  const [playbackUrl, setPlaybackUrl] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);

  const recorderRef = useRef<Recorder | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const rafRef = useRef<number | null>(null);

  const refreshDevices = async () => {
    setLoadingDevices(true);
    setDeviceError(null);
    try {
      setDevices(await listMicDevices());
    } catch (e) {
      setDeviceError(String(e));
    } finally {
      setLoadingDevices(false);
    }
  };

  useEffect(() => {
    refreshDevices();
    return () => {
      stopMeter();
      if (playbackUrl) URL.revokeObjectURL(playbackUrl);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const stopMeter = () => {
    if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    rafRef.current = null;
    audioCtxRef.current?.close().catch(() => {});
    audioCtxRef.current = null;
  };

  const startMeter = (stream: MediaStream) => {
    const ctx = new AudioContext();
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 256;
    ctx.createMediaStreamSource(stream).connect(analyser);
    audioCtxRef.current = ctx;
    const data = new Uint8Array(analyser.frequencyBinCount);
    const tick = () => {
      analyser.getByteTimeDomainData(data);
      let sumSquares = 0;
      for (const v of data) {
        const centered = (v - 128) / 128;
        sumSquares += centered * centered;
      }
      const rms = Math.sqrt(sumSquares / data.length);
      setLevel(Math.min(1, rms * 4));
      rafRef.current = requestAnimationFrame(tick);
    };
    tick();
  };

  const startTest = async () => {
    setTestError(null);
    if (playbackUrl) {
      URL.revokeObjectURL(playbackUrl);
      setPlaybackUrl(null);
    }
    try {
      const rec = await startRecording(deviceId || undefined);
      recorderRef.current = rec;
      startMeter(rec.stream);
      setTesting(true);
    } catch (e) {
      setTestError(String(e));
    }
  };

  const stopTest = async () => {
    const rec = recorderRef.current;
    recorderRef.current = null;
    setTesting(false);
    stopMeter();
    setLevel(0);
    if (!rec) return;
    try {
      const blob = await rec.stop();
      setPlaybackUrl(URL.createObjectURL(blob));
    } catch (e) {
      setTestError(String(e));
    }
  };

  return (
    <div className="space-y-3 rounded-lg border border-border/60 p-3">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-medium text-muted">Microphone</span>
        <button
          type="button"
          onClick={refreshDevices}
          disabled={loadingDevices}
          className="inline-flex items-center gap-1.5 rounded-lg border border-border px-2.5 py-1 text-xs font-medium text-fg hover:bg-accent disabled:opacity-50"
        >
          <RefreshCw size={12} className={loadingDevices ? "animate-spin" : ""} />
          Refresh
        </button>
      </div>

      <Combobox
        value={deviceId}
        onChange={onDeviceChange}
        placeholder="System default"
        options={[
          { value: "", label: "System default" },
          ...devices.map((d) => ({
            value: d.deviceId,
            label: d.label || `Microphone ${d.deviceId.slice(0, 6)}`,
          })),
        ]}
      />
      {deviceError && <span className="block text-xs text-red-500">{deviceError}</span>}

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={testing ? stopTest : startTest}
          className={cn(
            "inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-medium hover:bg-accent",
            testing ? "border-red-500 text-red-500" : "border-border text-fg",
          )}
        >
          {testing ? <Square size={13} /> : <Mic size={13} />}
          {testing ? "Stop test" : "Test microphone"}
        </button>

        {testing && (
          <div className="h-2 flex-1 overflow-hidden rounded-full bg-border">
            <div
              className="h-full rounded-full bg-primary transition-[width] duration-75"
              style={{ width: `${Math.round(level * 100)}%` }}
            />
          </div>
        )}

        {!testing && playbackUrl && (
          <span className="inline-flex items-center gap-1.5 text-xs text-muted">
            <Play size={12} />
            Playback ready
          </span>
        )}
      </div>

      {playbackUrl && !testing && (
        // eslint-disable-next-line jsx-a11y/media-has-caption
        <audio className="h-8 w-full" src={playbackUrl} controls />
      )}
      {testError && <span className="block text-xs text-red-500">{testError}</span>}
    </div>
  );
}
