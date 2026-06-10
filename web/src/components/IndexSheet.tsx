import { useEffect, useState } from 'react'
import { query } from '@/lib/ws'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'

/** Statut de l'index de recherche (parité Ctrl+I) — texte brut du cœur. */
export function IndexSheet({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const [text, setText] = useState<string>('')

  useEffect(() => {
    if (!open) return
    setText('Chargement…')
    query<{ text: string }>({ q: 'index_status' })
      .then((res) => setText(res.text))
      .catch((err) => setText(`Erreur : ${err}`))
  }, [open])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent side="right" aria-describedby={undefined}>
        <DialogTitle className="font-display text-2xl italic">Index de recherche</DialogTitle>
        <pre className="mt-4 whitespace-pre-wrap font-mono text-[0.75rem] leading-relaxed text-parchment-300">
          {text}
        </pre>
      </DialogContent>
    </Dialog>
  )
}
