import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/** 12345 → « 12.3K », 1234567 → « 1.2M ». */
export function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`
  return String(n)
}

/** Horodatage relatif compact (« il y a 3 min »). */
export function fmtAgo(ms: number): string {
  if (!ms) return ''
  const delta = Date.now() - ms
  const min = Math.floor(delta / 60_000)
  if (min < 1) return 'à l’instant'
  if (min < 60) return `il y a ${min} min`
  const hours = Math.floor(min / 60)
  if (hours < 24) return `il y a ${hours} h`
  return `il y a ${Math.floor(hours / 24)} j`
}

/** Icône (emoji) par type de panneau — héritée de l'esprit TUI. */
export function panelIcon(kind: string): string {
  const icons: Record<string, string> = {
    conversation: '📜',
    todo: '🪓',
    library: '📚',
    overview: '🌍',
    system: '🌍',
    tree: '🌲',
    memory: '✨',
    spine: '🦴',
    entities: '📦',
    queue: '📄',
    scratchpad: '🪶',
    callback: '🦴',
    file: '📄',
    git: '🌿',
    git_result: '🌿',
    github_result: '🐙',
    grep: '🔍',
    glob: '🔍',
    console: '🖥️',
    logs: '🗒️',
    tools: '🛠️',
    skill: '🎯',
    conversation_history: '🗂️',
    entity_result: '🗃️',
    'chat-dashboard': '💬',
  }
  return icons[kind] ?? '📄'
}

/** Langage Shiki déduit d'un chemin de fichier. */
export function langFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    rs: 'rust', ts: 'typescript', tsx: 'tsx', js: 'javascript', jsx: 'jsx',
    py: 'python', go: 'go', java: 'java', c: 'c', h: 'c', cpp: 'cpp', hpp: 'cpp',
    sh: 'bash', bash: 'bash', json: 'json', yaml: 'yaml', yml: 'yaml',
    toml: 'toml', md: 'markdown', html: 'html', css: 'css', sql: 'sql',
  }
  return map[ext] ?? 'text'
}
