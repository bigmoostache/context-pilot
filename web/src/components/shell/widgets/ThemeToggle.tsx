import { Moon, Sun } from "lucide-react"
import { useTheme } from "@/lib/providers/theme"
import { cn } from "@/lib/utils"

/**
 * macOS-style segmented light/dark switch. Two pills (sun · moon); the active
 * palette is highlighted. Reads/writes the global theme context.
 */
export function ThemeToggle() {
  const { theme, setTheme } = useTheme()
  return (
    <div className="flex items-center gap-0.5 rounded-full border border-border bg-muted/60 p-0.5">
      <Seg active={theme === "light"} onClick={() => setTheme("light")} label="Light">
        <Sun className="size-3.5" />
      </Seg>
      <Seg active={theme === "dark"} onClick={() => setTheme("dark")} label="Dark">
        <Moon className="size-3.5" />
      </Seg>
    </div>
  )
}

function Seg({
  active,
  onClick,
  label,
  children,
}: {
  active: boolean
  onClick: () => void
  label: string
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "flex size-6 items-center justify-center rounded-full transition-all",
        active
          ? "card-shadow bg-card text-foreground"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
    </button>
  )
}
