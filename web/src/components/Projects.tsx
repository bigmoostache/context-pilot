import { useEffect, useState } from 'react'
import { Anchor, Archive, FolderGit2, FolderOpen, GitBranch, LogOut, Plus, Settings2, Trash2 } from 'lucide-react'
import { useNestor } from '@/lib/store'
import { archiveProject, createProject, deleteProject, fetchProjects, logout, switchProject } from '@/lib/ws'
import { cn, fmtAgo } from '@/lib/utils'
import type { ProjectInfo } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'

/** Sélecteur de projets : chaque projet = un workspace sur la Pi avec sa
    propre session. Ouvrir le projet actif est instantané ; en ouvrir un
    autre redémarre le cœur dedans (~3 s, reconnexion automatique). */
export function Projects() {
  const setScreen = useNestor((s) => s.setScreen)
  const [projects, setProjects] = useState<ProjectInfo[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [createOpen, setCreateOpen] = useState(false)
  const [deleting, setDeleting] = useState<ProjectInfo | null>(null)

  async function refresh() {
    try {
      const res = await fetchProjects()
      setProjects(res.projects)
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Erreur'
      // Système de projets désactivé (pas de --projects-dir) : session unique.
      if (msg.includes('projects disabled')) {
        setScreen('shell')
        return
      }
      setError(msg)
    }
  }
  useEffect(() => {
    void refresh()
  }, [])

  async function open(project: ProjectInfo) {
    setError(null)
    if (project.current) {
      setScreen('shell')
      return
    }
    try {
      await switchProject(project.name) // l'overlay App prend le relais
      setScreen('shell')
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Erreur')
    }
  }

  async function archive(project: ProjectInfo) {
    setError(null)
    try {
      await archiveProject(project.name)
      void refresh()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Erreur')
    }
  }

  return (
    <div className="flex h-full flex-col items-center overflow-y-auto p-8">
      <header className="relative mb-10 mt-6 w-full max-w-3xl text-center animate-rise">
        <div className="mx-auto mb-3 flex size-12 items-center justify-center rounded-2xl border border-brass-600/40 bg-coal-900 shadow-[0_0_36px_-8px] shadow-brass-500/30">
          <Anchor className="size-6 text-brass-400" />
        </div>
        <h1 className="font-display text-4xl italic">Nestor</h1>
        <p className="mt-1 text-sm text-parchment-500">Choisis un projet — chacun a son atelier sur la Pi.</p>
        <button
          onClick={() => setScreen('settings')}
          title="Paramètres généraux (WiFi, clés API, système)"
          className="absolute right-0 top-0 rounded-md p-2 text-parchment-500 hover:bg-coal-800 hover:text-parchment-100 cursor-pointer"
        >
          <Settings2 className="size-5" />
        </button>
      </header>

      {error && <p className="mb-4 max-w-xl text-center text-sm text-ember-400">{error}</p>}

      <div className="grid w-full max-w-3xl grid-cols-1 gap-3 sm:grid-cols-2">
        {(projects ?? []).map((project, i) => (
          <ProjectCard
            key={project.name}
            project={project}
            delay={i}
            onOpen={() => void open(project)}
            onArchive={() => void archive(project)}
            onDelete={() => setDeleting(project)}
          />
        ))}

        {/* Nouveau projet */}
        <button
          onClick={() => setCreateOpen(true)}
          style={{ animationDelay: `${Math.min((projects?.length ?? 0) * 60, 400)}ms` }}
          className="flex min-h-28 items-center justify-center gap-2 rounded-xl border border-dashed border-coal-600 text-parchment-500 transition-colors hover:border-brass-600 hover:text-brass-300 animate-rise cursor-pointer"
        >
          <Plus className="size-4" />
          Nouveau projet
        </button>
      </div>

      {projects !== null && projects.length === 0 && (
        <p className="mt-6 text-sm text-parchment-700">Aucun projet — crée le premier.</p>
      )}

      <footer className="mt-auto pt-8">
        <Button variant="ghost" size="sm" onClick={logout}>
          <LogOut className="size-3.5" /> Se déconnecter
        </Button>
      </footer>

      <CreateDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={(name) => {
          setCreateOpen(false)
          void refresh()
          // Ouvre directement le projet fraîchement créé.
          void switchProject(name).then(() => setScreen('shell')).catch(() => {})
        }}
      />
      <DeleteDialog project={deleting} onClose={() => setDeleting(null)} onDeleted={() => void refresh()} />
    </div>
  )
}

function ProjectCard({
  project,
  delay,
  onOpen,
  onArchive,
  onDelete,
}: {
  project: ProjectInfo
  delay: number
  onOpen: () => void
  onArchive: () => void
  onDelete: () => void
}) {
  return (
    <div
      style={{ animationDelay: `${Math.min(delay * 60, 400)}ms` }}
      className={cn(
        'group relative rounded-xl border bg-coal-900/60 p-4 text-left transition-colors animate-rise',
        project.current ? 'border-brass-600/50 shadow-[0_0_30px_-12px] shadow-brass-500/40' : 'border-coal-700 hover:border-coal-600',
      )}
    >
      <button onClick={onOpen} className="block w-full text-left cursor-pointer">
        <div className="flex items-center gap-2">
          {project.has_git ? (
            <FolderGit2 className="size-4 text-tide-400" />
          ) : (
            <FolderOpen className="size-4 text-parchment-500" />
          )}
          <span className="font-medium text-parchment-100">{project.name}</span>
          {project.current && (
            <span className="rounded-full border border-brass-600/50 bg-brass-500/10 px-2 py-0.5 font-mono text-[0.6rem] uppercase tracking-wider text-brass-300">
              à bord
            </span>
          )}
        </div>
        <div className="mt-1.5 flex items-center gap-2 font-mono text-[0.7rem] text-parchment-700">
          {project.has_git && <GitBranch className="size-3" />}
          <span>{project.last_active_ms ? `actif ${fmtAgo(project.last_active_ms)}` : 'jamais ouvert'}</span>
        </div>
      </button>

      {/* Actions (apparaissent au survol ; jamais sur le projet actif) */}
      {!project.current && (
        <div className="absolute right-2.5 top-2.5 flex gap-1 opacity-0 transition-opacity group-hover:opacity-100">
          <button
            onClick={onArchive}
            title="Archiver (réversible en SSH)"
            className="rounded-md p-1.5 text-parchment-500 hover:bg-coal-800 hover:text-parchment-100 cursor-pointer"
          >
            <Archive className="size-3.5" />
          </button>
          <button
            onClick={onDelete}
            title="Supprimer définitivement"
            className="rounded-md p-1.5 text-parchment-500 hover:bg-ember-400/15 hover:text-ember-400 cursor-pointer"
          >
            <Trash2 className="size-3.5" />
          </button>
        </div>
      )}
    </div>
  )
}

/** Création : nom + URL git optionnelle (le clone dure le temps qu'il dure). */
function CreateDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  onCreated: (name: string) => void
}) {
  const [name, setName] = useState('')
  const [gitUrl, setGitUrl] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const nameOk = /^[A-Za-z0-9_-]{1,64}$/.test(name)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    if (!nameOk || busy) return
    setBusy(true)
    setError(null)
    try {
      await createProject(name, gitUrl || undefined)
      setName('')
      setGitUrl('')
      onCreated(name)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Erreur')
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent>
        <DialogTitle className="font-display text-2xl italic">Nouveau projet</DialogTitle>
        <DialogDescription>Un dossier vierge sur la Pi — ou un dépôt git à cloner dedans.</DialogDescription>
        <form onSubmit={submit} className="mt-4 space-y-3">
          <div>
            <label className="mb-1 block text-xs font-medium uppercase tracking-wider text-parchment-500">Nom</label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              placeholder="mon-projet"
              className="h-10 w-full rounded-md border border-coal-700 bg-coal-900 px-3 font-mono text-sm text-parchment-100 placeholder:text-parchment-700 focus:outline-2 focus:outline-brass-400"
            />
            {name && !nameOk && (
              <p className="mt-1 text-xs text-ember-400">Lettres, chiffres, tirets et underscores seulement.</p>
            )}
          </div>
          <div>
            <label className="mb-1 block text-xs font-medium uppercase tracking-wider text-parchment-500">
              URL git <span className="normal-case text-parchment-700">(optionnel)</span>
            </label>
            <input
              value={gitUrl}
              onChange={(e) => setGitUrl(e.target.value)}
              placeholder="https://github.com/… ou git@…"
              className="h-10 w-full rounded-md border border-coal-700 bg-coal-900 px-3 font-mono text-sm text-parchment-100 placeholder:text-parchment-700 focus:outline-2 focus:outline-brass-400"
            />
          </div>
          {error && <p className="text-sm text-ember-400">{error}</p>}
          <div className="flex justify-end gap-2 pt-1">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
              Annuler
            </Button>
            <Button type="submit" disabled={!nameOk || busy}>
              {busy ? (gitUrl ? 'Clonage…' : 'Création…') : 'Créer et ouvrir'}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** Suppression définitive : confirmation par saisie du nom. */
function DeleteDialog({
  project,
  onClose,
  onDeleted,
}: {
  project: ProjectInfo | null
  onClose: () => void
  onDeleted: () => void
}) {
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setConfirm('')
    setError(null)
  }, [project?.name])

  if (!project) return null

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    try {
      await deleteProject(project!.name, confirm)
      onClose()
      onDeleted()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Erreur')
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogTitle className="font-display text-2xl italic text-ember-400">Supprimer « {project.name} »</DialogTitle>
        <DialogDescription>
          Suppression définitive du workspace et de toute sa session (conversation, mémoire, fichiers).
          Tape le nom du projet pour confirmer.
        </DialogDescription>
        <form onSubmit={submit} className="mt-4 space-y-3">
          <input
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            autoFocus
            placeholder={project.name}
            className="h-10 w-full rounded-md border border-coal-700 bg-coal-900 px-3 font-mono text-sm text-parchment-100 placeholder:text-parchment-700 focus:outline-2 focus:outline-ember-400"
          />
          {error && <p className="text-sm text-ember-400">{error}</p>}
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={onClose}>
              Annuler
            </Button>
            <Button type="submit" variant="danger" disabled={confirm !== project.name}>
              Supprimer définitivement
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
