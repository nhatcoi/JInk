import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { accelIssue, acceleratorFromEvent, formatAccelerator } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";

type Props = {
  value: string;
  onChange: (accel: string) => void;
  className?: string;
};

/** Click, then press a key combo — captures it as an accelerator string. */
export function KeyCapture({ value, onChange, className }: Props) {
  const [capturing, setCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!capturing) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const accel = acceleratorFromEvent(e);
      if (!accel) return; // only modifiers held so far — keep listening
      const issue = accelIssue(accel);
      if (issue) {
        setError(issue);
        return;
      }
      onChange(accel);
      setCapturing(false);
      setError(null);
    };
    // Capture phase: beat the popup's own keydown handlers while armed.
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [capturing, onChange]);

  useEffect(() => {
    if (!capturing) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setCapturing(false);
        setError(null);
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [capturing]);

  return (
    <div ref={rootRef} className={cn("flex items-center gap-1.5", className)}>
      <button
        type="button"
        onClick={() => {
          setError(null);
          setCapturing(true);
        }}
        className={cn(
          "h-9 flex-1 rounded-lg border px-3 text-left text-sm outline-none",
          capturing
            ? "border-ring bg-input text-muted ring-2 ring-ring"
            : "border-border bg-input text-fg hover:bg-accent",
        )}
      >
        {capturing ? (error ?? "Press a key combo…") : formatAccelerator(value)}
      </button>
      {value && !capturing && (
        <button
          type="button"
          onClick={() => onChange("")}
          title="Clear shortcut"
          className="text-muted hover:text-fg"
        >
          <X size={14} />
        </button>
      )}
    </div>
  );
}
