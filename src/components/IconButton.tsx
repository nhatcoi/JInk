import { forwardRef, type ButtonHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  active?: boolean;
  label?: string;
};

/** Compact toolbar icon button, shadcn "ghost" flavour. */
export const IconButton = forwardRef<HTMLButtonElement, Props>(
  ({ className, active, label, ...props }, ref) => (
    <button
      ref={ref}
      type="button"
      title={label}
      aria-label={label}
      className={cn(
        "inline-flex h-8 w-8 items-center justify-center rounded-lg text-muted transition-colors",
        "hover:bg-accent hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        "disabled:opacity-40 disabled:pointer-events-none",
        active && "bg-accent text-fg",
        className,
      )}
      {...props}
    />
  ),
);
IconButton.displayName = "IconButton";
