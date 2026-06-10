import { useEffect, useState } from 'react'
import { useNestor } from '@/lib/store'
import { send } from '@/lib/ws'
import { cn } from '@/lib/utils'
import { Dialog, DialogContent, DialogTitle, DialogDescription } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

interface LocalAnswer {
  selected: number[]
  other: string
}

/** Formulaire `ask_user_question` : l'agent pose des questions, on répond
    d'un clic — l'équivalent web du formulaire TUI. */
export function QuestionDialog() {
  const form = useNestor((s) => s.state?.question_form)
  const [answers, setAnswers] = useState<LocalAnswer[]>([])

  useEffect(() => {
    setAnswers(form?.questions.map(() => ({ selected: [], other: '' })) ?? [])
  }, [form?.tool_use_id, form?.questions])

  if (!form) return null

  function toggle(qIdx: number, optIdx: number, multi: boolean) {
    setAnswers((prev) =>
      prev.map((ans, i) => {
        if (i !== qIdx) return ans
        if (multi) {
          const has = ans.selected.includes(optIdx)
          return { ...ans, selected: has ? ans.selected.filter((s) => s !== optIdx) : [...ans.selected, optIdx] }
        }
        return { ...ans, selected: [optIdx] }
      }),
    )
  }

  function setOther(qIdx: number, text: string) {
    setAnswers((prev) => prev.map((ans, i) => (i === qIdx ? { ...ans, other: text } : ans)))
  }

  const complete = answers.every((ans) => ans.selected.length > 0 || ans.other.trim().length > 0)

  function submit() {
    send({
      cmd: 'answer_question',
      tool_use_id: form!.tool_use_id,
      answers: answers.map((ans) => ({
        selected: ans.selected,
        ...(ans.other.trim() ? { other_text: ans.other.trim() } : {}),
      })),
    })
  }

  return (
    <Dialog open onOpenChange={(open) => !open && send({ cmd: 'dismiss_question', tool_use_id: form.tool_use_id })}>
      <DialogContent className="max-w-xl max-h-[85vh] overflow-y-auto">
        <DialogTitle className="font-display text-2xl italic">Nestor a besoin de toi</DialogTitle>
        <DialogDescription>Réponds pour débloquer la suite du travail.</DialogDescription>

        <div className="mt-4 space-y-6">
          {form.questions.map((question, qIdx) => (
            <div key={question.header + qIdx}>
              <div className="mb-0.5 font-mono text-[0.65rem] uppercase tracking-widest text-brass-400">
                {question.header}
              </div>
              <p className="mb-2 text-sm text-parchment-100">{question.text}</p>
              <div className="space-y-1.5">
                {question.options.map((opt, optIdx) => {
                  const checked = answers[qIdx]?.selected.includes(optIdx) ?? false
                  return (
                    <button
                      key={opt.label}
                      onClick={() => toggle(qIdx, optIdx, question.multi_select)}
                      className={cn(
                        'block w-full rounded-lg border px-3 py-2 text-left text-sm transition-colors cursor-pointer',
                        checked
                          ? 'border-brass-500 bg-brass-500/10 text-parchment-100'
                          : 'border-coal-700 bg-coal-850 text-parchment-300 hover:border-coal-600',
                      )}
                    >
                      <span className="font-medium">{opt.label}</span>
                      {opt.description && <span className="mt-0.5 block text-xs text-parchment-500">{opt.description}</span>}
                    </button>
                  )
                })}
                <input
                  type="text"
                  placeholder="Autre…"
                  value={answers[qIdx]?.other ?? ''}
                  onChange={(e) => setOther(qIdx, e.target.value)}
                  className="h-9 w-full rounded-lg border border-coal-700 bg-coal-900 px-3 text-sm text-parchment-100 placeholder:text-parchment-700 focus:outline-2 focus:outline-brass-400"
                />
              </div>
            </div>
          ))}
        </div>

        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" onClick={() => send({ cmd: 'dismiss_question', tool_use_id: form.tool_use_id })}>
            Ignorer
          </Button>
          <Button onClick={submit} disabled={!complete}>
            Répondre
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
