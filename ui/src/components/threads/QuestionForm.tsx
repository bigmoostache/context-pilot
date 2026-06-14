import { useState } from "react"
import { Check } from "lucide-react"
import type { ThreadQuestion } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Embedded thread question form — Context Pilot's signature way to ask the
 * user a structured question inside a conversation. Decorative (design only)
 * but fully interactive so the selection state feels real.
 */
export function QuestionForm({ q }: { q: ThreadQuestion }) {
  const [picked, setPicked] = useState<number[]>([])
  const toggle = (i: number) => {
    setPicked((cur) => {
      if (q.multi) return cur.includes(i) ? cur.filter((x) => x !== i) : [...cur, i]
      return cur.includes(i) ? [] : [i]
    })
  }

  return (
    <div className="mt-2 max-w-[88%] overflow-hidden rounded-[5px] border border-[var(--interactive)]/35 bg-[oklch(0.185_0.012_200)]">
      <div className="flex items-center gap-1.5 border-b border-[var(--interactive)]/25 bg-[var(--interactive)]/10 px-3 py-1.5">
        <span className="size-1.5 animate-pulse rounded-full bg-[var(--interactive)] shadow-[0_0_5px_var(--interactive)]" />
        <span className="text-[10px] uppercase tracking-[0.16em] text-[var(--interactive)]">
          question · awaiting you
        </span>
        {q.multi && (
          <span className="ml-auto rounded-[2px] bg-[var(--interactive)]/15 px-1 text-[9px] uppercase tracking-wider text-[var(--interactive)]">
            multi-select
          </span>
        )}
      </div>

      <div className="px-3 py-2.5">
        <p className="mb-2 font-sans text-[13px] leading-relaxed text-foreground/90">{q.prompt}</p>

        <div className="flex flex-col gap-1.5">
          {q.options.map((opt, i) => {
            const on = picked.includes(i)
            return (
              <button
                key={i}
                onClick={() => toggle(i)}
                className={cn(
                  "group flex items-center gap-2.5 rounded-[3px] border px-2.5 py-1.5 text-left text-[12px] transition-colors",
                  on
                    ? "border-[var(--interactive)] bg-[var(--interactive)]/12 text-foreground"
                    : "border-border bg-[oklch(0.2_0.008_75)] text-foreground/75 hover:border-[var(--interactive)]/50 hover:text-foreground",
                )}
              >
                <span
                  className={cn(
                    "flex size-4 shrink-0 items-center justify-center border",
                    q.multi ? "rounded-[3px]" : "rounded-full",
                    on
                      ? "border-[var(--interactive)] bg-[var(--interactive)] text-[oklch(0.16_0.02_200)]"
                      : "border-muted-foreground/50",
                  )}
                >
                  {on && <Check className="size-2.5" strokeWidth={3} />}
                </span>
                {opt}
              </button>
            )
          })}

          {q.allowOther && (
            <div className="flex items-center gap-2.5 rounded-[3px] border border-dashed border-border bg-[oklch(0.2_0.008_75)] px-2.5 py-1.5 text-[12px] text-muted-foreground/60">
              <span className="flex size-4 shrink-0 items-center justify-center rounded-full border border-muted-foreground/40" />
              other…
            </div>
          )}
        </div>

        <div className="mt-2.5 flex items-center justify-between">
          <span className="text-[10px] text-muted-foreground/50">
            {picked.length > 0 ? `${picked.length} selected` : "no selection"}
          </span>
          <button
            className={cn(
              "rounded-[3px] px-3 py-1 text-[11px] font-semibold tracking-wide transition-colors",
              picked.length > 0
                ? "bg-[var(--interactive)] text-[oklch(0.16_0.02_200)] hover:brightness-110"
                : "cursor-not-allowed bg-[oklch(0.24_0.008_75)] text-muted-foreground/50",
            )}
          >
            Submit
          </button>
        </div>
      </div>
    </div>
  )
}
