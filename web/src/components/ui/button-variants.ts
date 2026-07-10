import { cva } from "class-variance-authority"

// Split out of `button.tsx` so that file only exports components (React Fast
// Refresh — react-refresh/only-export-components). Import `buttonVariants` from
// here when you need the class string on a non-`<Button>` element (e.g. a
// Base UI render prop or an anchor styled as a button).
export const buttonVariants = cva(
  "group/button inline-flex shrink-0 items-center justify-center rounded-lg border border-transparent bg-clip-padding text-sm font-medium whitespace-nowrap transition-all outline-none select-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 active:not-aria-[haspopup]:translate-y-px disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground hover:bg-primary/80",
        outline:
          "border-border bg-background hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground dark:border-input dark:bg-input/30 dark:hover:bg-input/50",
        secondary:
          "bg-secondary text-secondary-foreground hover:bg-[color-mix(in_oklch,var(--color-secondary),var(--foreground)_5%)] aria-expanded:bg-secondary aria-expanded:text-secondary-foreground",
        ghost:
          "hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground dark:hover:bg-muted/50",
        destructive:
          "bg-destructive/10 text-destructive hover:bg-destructive/20 focus-visible:border-destructive/40 focus-visible:ring-destructive/20 dark:bg-destructive/20 dark:hover:bg-destructive/30 dark:focus-visible:ring-destructive/40",
        link: "text-primary underline-offset-4 hover:underline",
        // App-native accent CTA. This cockpit's primary action colour is
        // `--signal` (not shadcn's `--primary`), so `signal` is the variant that
        // matches the hand-rolled `bg-[var(--signal)]` submit buttons across the
        // dialogs/auth forms — adopting it keeps their look while adding the
        // shared focus-visible ring + disabled handling (M27).
        signal:
          "bg-[var(--signal)] text-[var(--primary-foreground)] transition-[filter] hover:brightness-105",
        // The app's OTHER accent — `--interactive` — used for the "spawn / open"
        // family of CTAs (New agent, Open, Send…). Same shape as `signal`, so the
        // hand-rolled `bg-[var(--interactive)]` buttons adopt it verbatim (M27).
        interactive:
          "bg-[var(--interactive)] text-[var(--primary-foreground)] transition-[filter] hover:brightness-105",
        // The recurring "pill" secondary: a quiet, muted control that lights up
        // when it is the active/selected one (drive via `aria-pressed` on a
        // toggle or `aria-current` on a nav item). Pairs with `size="none"` for
        // the bespoke geometries these pills use (M27).
        pill: "font-medium text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground aria-pressed:bg-card aria-pressed:text-foreground aria-pressed:card-shadow aria-current:bg-card aria-current:text-foreground aria-current:card-shadow",
      },
      size: {
        default:
          "h-8 gap-1.5 px-2.5 has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2",
        // No intrinsic height/padding — for pills and other bespoke geometries
        // that bring their own sizing via className.
        none: "",
        xs: "h-6 gap-1 rounded-[min(var(--radius-md),10px)] px-2 text-xs in-data-[slot=button-group]:rounded-lg has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3",
        sm: "h-7 gap-1 rounded-[min(var(--radius-md),12px)] px-2.5 text-[0.8rem] in-data-[slot=button-group]:rounded-lg has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3.5",
        lg: "h-9 gap-1.5 px-2.5 has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2",
        icon: "size-8",
        "icon-xs":
          "size-6 rounded-[min(var(--radius-md),10px)] in-data-[slot=button-group]:rounded-lg [&_svg:not([class*='size-'])]:size-3",
        "icon-sm":
          "size-7 rounded-[min(var(--radius-md),12px)] in-data-[slot=button-group]:rounded-lg",
        "icon-lg": "size-9",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
)
