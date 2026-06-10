import { useEffect, useState } from 'react'
import { useNestor } from '@/lib/store'
import { connect, getToken } from '@/lib/ws'
import { Login } from '@/components/Login'
import { Projects } from '@/components/Projects'
import { Settings } from '@/components/Settings'
import { Sidebar } from '@/components/Sidebar'
import { Chat } from '@/components/Chat'
import { Composer } from '@/components/Composer'
import { PanelView } from '@/components/PanelView'
import { StatusBar } from '@/components/StatusBar'
import { ConfigSheet } from '@/components/ConfigSheet'
import { Palette } from '@/components/Palette'
import { QuestionDialog } from '@/components/QuestionDialog'
import { IndexSheet } from '@/components/IndexSheet'

export default function App() {
  const conn = useNestor((s) => s.conn)
  const hasState = useNestor((s) => s.state !== null)
  const screen = useNestor((s) => s.screen)
  const switchingTo = useNestor((s) => s.switchingTo)
  const [configOpen, setConfigOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [indexOpen, setIndexOpen] = useState(false)

  useEffect(() => {
    if (getToken()) connect()
  }, [])

  // Raccourcis globaux : les power-users gardent leurs réflexes TUI.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key === 'p') {
        e.preventDefault()
        setPaletteOpen((open) => !open)
      } else if ((e.ctrlKey || e.metaKey) && e.key === 'h') {
        e.preventDefault()
        setConfigOpen((open) => !open)
      } else if ((e.ctrlKey || e.metaKey) && e.key === 'i') {
        e.preventDefault()
        setIndexOpen((open) => !open)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  if (conn === 'login') return <Login />

  // Bascule de projet : le cœur redémarre dans le nouveau workspace.
  if (switchingTo !== null) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2">
        <p className="font-display text-3xl italic text-brass-300 animate-pulse">Cap sur « {switchingTo} »…</p>
        <p className="text-sm text-parchment-700">Nestor change d’atelier, reconnexion automatique.</p>
      </div>
    )
  }

  if (screen === 'projects') return <Projects />
  if (screen === 'settings') return <Settings />

  if (!hasState) {
    return (
      <div className="flex h-full items-center justify-center">
        <p className="font-display text-2xl italic text-parchment-500 animate-pulse">
          {conn === 'offline' ? 'Pi hors de portée — reconnexion…' : 'Levée de l’ancre…'}
        </p>
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1">
        <Sidebar onOpenConfig={() => setConfigOpen(true)} />
        <main className="flex min-w-0 flex-1 flex-col">
          <Chat />
          <Composer />
        </main>
        <PanelView />
      </div>
      <StatusBar />

      <ConfigSheet open={configOpen} onOpenChange={setConfigOpen} />
      <Palette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        onOpenConfig={() => setConfigOpen(true)}
        onOpenIndex={() => setIndexOpen(true)}
      />
      <IndexSheet open={indexOpen} onOpenChange={setIndexOpen} />
      <QuestionDialog />
    </div>
  )
}
