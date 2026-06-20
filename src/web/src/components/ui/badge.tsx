import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";
import { cn } from "../../lib/utils";

const badgeVariants = cva(
  "inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium transition-colors",
  {
    variants: {
      variant: {
        default:
          "border-transparent bg-[var(--primary-soft)] text-[var(--primary-text)]",
        secondary:
          "border-transparent bg-[var(--surface-muted)] text-[var(--muted-foreground)]",
        outline: "border-[var(--border)] text-[var(--foreground)]",
        destructive:
          "border-transparent bg-[var(--destructive-soft)] text-[var(--destructive-text)]"
      }
    },
    defaultVariants: {
      variant: "default"
    }
  }
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <div className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge };
