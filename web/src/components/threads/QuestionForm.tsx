import { useState } from "react"
import { Check } from "lucide-react"
import type { ThreadQuestion } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Embedded thread question form — Context Pilot's signature way to ask the
 * user a structured question inside a conversation. Shows the header + prompt,
 * interactive option selection, and a read-only "answered" state after submit.
 */
export function QuestionForm({
  q,
  onSubmit,
}: {
  q: ThreadQuestion
  onSubmit?: (answer: string) => void
}) {
  const [picked, setPicked] = useState<number[]>([])
  const [submitted, setSubmitted] = useState(false)
  const options = q.options
  const isAnswered = submitted || !!q.answered
  const answeredLabels = q.answered ?? picked.map((i) => options[i] ?? "")

  const toggle = (i: number) => {
    if (isAnswered) return
    setPicked((cur) => {
      if (q.multi) return cur.includes(i) ? cur.filter((x) => x !== i) : [...cur, i]
      return cur.includes(i) ? [] : [i]
    })
  }

  const handleSubmit = () => {
    if (picked.length === 0 || !onSubmit) return
    const answers = picked.map((i) => options[i])
    setSubmitted(true)
    onSubmit(answers.join(", "))
  }

  return (
    <div className="card-shadow mt-2 max-w-[88%] overflow-hidden rounded-xl border border-(--interactive)/30 bg-card">
      <div className="flex items-center gap-2 border-b border-border bg-(--interactive)/8 px-3 py-2">
        <span className="size-1.5 rounded-full bg-(--interactive)" />
        <span className="text-[11px] font-medium text-(--interactive)">
          {isAnswered ? "Question · answered" : "Question · awaiting you"}
        </span>
        {q.multi && !isAnswered && (
          <span className="ml-auto rounded-full bg-(--interactive)/12 px-2 py-0.5 text-[10px] font-medium text-(--interactive)">
            Multi-select
          </span>
        )}
      </div>

      <div className="p-3">
        {q.header && (
          <p className="mb-1 text-[11px] font-semibold tracking-wide text-muted-foreground/70 uppercase">
            {q.header}
          </p>
        )}
        <p className="mb-2.5 text-[13px] leading-relaxed text-foreground/90">{q.prompt}</p>

        <div className="flex flex-col gap-1.5">
          {options.map((opt, i) => {
            const on = isAnswered ? answeredLabels.includes(opt) : picked.includes(i)
            return (
              <button
                key={i}
                onClick={() => toggle(i)}
                disabled={isAnswered}
                className={cn(
                  "flex items-center gap-2.5 rounded-lg border px-3 py-2 text-left text-[12.5px] transition-colors",
                  isAnswered
                    ? on
                      ? "border-(--interactive) bg-(--interactive)/10 text-foreground"
                      : "border-border bg-muted/20 text-muted-foreground/50"
                    : on
                      ? "border-(--interactive) bg-(--interactive)/10 text-foreground"
                      : "border-border bg-muted/40 text-foreground/75 hover:border-(--interactive)/50 hover:text-foreground",
                )}
              >
                <span
                  className={cn(
                    "flex size-4 shrink-0 items-center justify-center border",
                    q.multi ? "rounded-[4px]" : "rounded-full",
                    on
                      ? "border-(--interactive) bg-(--interactive) text-(--primary-foreground)"
                      : "border-muted-foreground/50",
                  )}
                >
                  {on && <Check className="size-2.5" strokeWidth={3} />}
                </span>
                {opt}
              </button>
            )
          })}
        </div>

        {!isAnswered && (
          <div className="mt-3 flex items-center justify-between">
            <span className="text-[11px] text-muted-foreground/60">
              {picked.length > 0 ? `${picked.length} selected` : "No selection"}
            </span>
            <button
              onClick={handleSubmit}
              disabled={picked.length === 0}
              className={cn(
                "rounded-lg px-3.5 py-1.5 text-[12px] font-medium transition-[filter]",
                picked.length > 0
                  ? "bg-(--interactive) text-(--primary-foreground) hover:brightness-105"
                  : "cursor-not-allowed bg-muted text-muted-foreground/50",
              )}
            >
              Submit
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
