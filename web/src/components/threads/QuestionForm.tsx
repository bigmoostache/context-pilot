import { useState } from "react"
import { Check } from "lucide-react"
import type { ThreadQuestion } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Embedded thread question form — Context Pilot's signature way to ask the
 * user a structured question inside a conversation. Decorative (design only)
 * but fully interactive so the selection state feels real.
 */
export function QuestionForm({ q, onSubmit }: { q: ThreadQuestion; onSubmit?: (answer: string) => void }) {
  const [picked, setPicked] = useState<number[]>([])
  const toggle = (i: number) => {
    setPicked((cur) => {
      if (q.multi) return cur.includes(i) ? cur.filter((x) => x !== i) : [...cur, i]
      return cur.includes(i) ? [] : [i]
    })
  }

  return (
    <div className="mt-2 max-w-[88%] overflow-hidden rounded-xl border border-[var(--interactive)]/30 bg-card card-shadow">
      <div className="flex items-center gap-2 border-b border-border bg-[var(--interactive)]/8 px-3 py-2">
        <span className="size-1.5 rounded-full bg-[var(--interactive)]" />
        <span className="text-[11px] font-medium text-[var(--interactive)]">
          Question · awaiting you
        </span>
        {q.multi && (
          <span className="ml-auto rounded-full bg-[var(--interactive)]/12 px-2 py-0.5 text-[10px] font-medium text-[var(--interactive)]">
            Multi-select
          </span>
        )}
      </div>

      <div className="px-3 py-3">
        <p className="mb-2.5 text-[13px] leading-relaxed text-foreground/90">{q.prompt}</p>

        <div className="flex flex-col gap-1.5">
          {q.options.map((opt, i) => {
            const on = picked.includes(i)
            return (
              <button
                key={i}
                onClick={() => toggle(i)}
                className={cn(
                  "flex items-center gap-2.5 rounded-lg border px-3 py-2 text-left text-[12.5px] transition-colors",
                  on
                    ? "border-[var(--interactive)] bg-[var(--interactive)]/10 text-foreground"
                    : "border-border bg-muted/40 text-foreground/75 hover:border-[var(--interactive)]/50 hover:text-foreground",
                )}
              >
                <span
                  className={cn(
                    "flex size-4 shrink-0 items-center justify-center border",
                    q.multi ? "rounded-[4px]" : "rounded-full",
                    on
                      ? "border-[var(--interactive)] bg-[var(--interactive)] text-[var(--primary-foreground)]"
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
            <div className="flex items-center gap-2.5 rounded-lg border border-dashed border-border bg-muted/40 px-3 py-2 text-[12.5px] text-muted-foreground/60">
              <span className="flex size-4 shrink-0 items-center justify-center rounded-full border border-muted-foreground/40" />
              Other…
            </div>
          )}
        </div>

        <div className="mt-3 flex items-center justify-between">
          <span className="text-[11px] text-muted-foreground/60">
            {picked.length > 0 ? `${picked.length} selected` : "No selection"}
          </span>
          <button
            onClick={() => {
              if (picked.length === 0 || !onSubmit) return
              const answers = picked.map((i) => q.options[i])
              onSubmit(answers.join(", "))
            }}
            disabled={picked.length === 0}
            className={cn(
              "rounded-lg px-3.5 py-1.5 text-[12px] font-medium transition-[filter]",
              picked.length > 0
                ? "bg-[var(--interactive)] text-[var(--primary-foreground)] hover:brightness-105"
                : "cursor-not-allowed bg-muted text-muted-foreground/50",
            )}
          >
            Submit
          </button>
        </div>
      </div>
    </div>
  )
}
