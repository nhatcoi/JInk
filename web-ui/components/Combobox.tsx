import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronsUpDown, Search } from "lucide-react";
import { cn } from "@/lib/utils";

export type ComboOption = { value: string; label: string };

type Props = {
  value: string;
  options: ComboOption[];
  onChange: (value: string) => void;
  placeholder?: string;
  /** Show a filter input at the top of the popover. */
  searchable?: boolean;
  className?: string;
};

/**
 * shadcn-style combobox: a button trigger + popover list with keyboard nav and
 * a check on the selected item. Self-contained (no cmdk/radix), themed via the
 * app's Tailwind tokens.
 */
export function Combobox({
  value,
  options,
  onChange,
  placeholder = "Select…",
  searchable = false,
  className,
}: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const selected = options.find((o) => o.value === value);
  const filtered = useMemo(() => {
    if (!searchable || !query) return options;
    const q = query.toLowerCase();
    return options.filter((o) => o.label.toLowerCase().includes(q));
  }, [options, query, searchable]);

  // Close on outside click / Escape.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node))
        setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(Math.max(0, options.findIndex((o) => o.value === value)));
      if (searchable) requestAnimationFrame(() => searchRef.current?.focus());
    }
  }, [open, options, value, searchable]);

  const pick = (v: string) => {
    onChange(v);
    setOpen(false);
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(filtered.length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const opt = filtered[active];
      if (opt) pick(opt.value);
    } else if (e.key === "Escape") {
      setOpen(false);
    }
  };

  return (
    <div ref={rootRef} className={cn("relative", className)}>
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex h-9 w-full items-center justify-between rounded-lg border border-border bg-input px-3 text-sm text-fg outline-none focus:ring-2 focus:ring-ring"
      >
        <span className={cn(!selected && "text-muted")}>
          {selected ? selected.label : placeholder}
        </span>
        <ChevronsUpDown size={15} className="ml-2 shrink-0 text-muted" />
      </button>

      {open && (
        <div
          role="listbox"
          onKeyDown={onKey}
          className="animate-pop absolute z-50 mt-1.5 w-full overflow-hidden rounded-lg border border-border bg-surface p-1 shadow-lg"
        >
          {searchable && (
            <div className="mb-1 flex items-center gap-2 border-b border-border px-2 pb-1.5">
              <Search size={14} className="text-muted" />
              <input
                ref={searchRef}
                value={query}
                onChange={(e) => {
                  setQuery(e.target.value);
                  setActive(0);
                }}
                onKeyDown={onKey}
                placeholder="Search…"
                className="h-7 w-full bg-transparent text-sm text-fg outline-none placeholder:text-muted"
              />
            </div>
          )}
          <div className="max-h-56 overflow-y-auto">
            {filtered.length === 0 && (
              <div className="px-2 py-2 text-sm text-muted">No results.</div>
            )}
            {filtered.map((o, i) => (
              <button
                key={o.value}
                type="button"
                role="option"
                aria-selected={o.value === value}
                onMouseEnter={() => setActive(i)}
                onClick={() => pick(o.value)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-fg",
                  i === active && "bg-accent",
                )}
              >
                <Check
                  size={15}
                  className={cn(
                    "shrink-0",
                    o.value === value ? "opacity-100" : "opacity-0",
                  )}
                />
                {o.label}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
