import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  Check,
  ChevronDown,
  Download,
  Loader2,
  Package,
  RefreshCw,
  Trash2,
} from "lucide-react"
import {
  deleteApiReleasesByTag,
  getApiReleases,
  postApiReleasesDownload,
  putApiReleasesArch,
  putApiReleasesSelect,
} from "@/lib/api/generated"
import type { ReleaseEntry } from "@/lib/api/generated"
import { cn } from "@/lib/utils"

const PAGE_SIZE = 5

/**
 * Admin-only release management pane (T427).
 *
 * Shows architecture selection, local/remote releases, and download/select/
 * delete actions. Data flows through the generated SDK → orchestrator REST →
 * {@link ReleaseStore} backend service.
 */
export function ReleasesPane() {
  const qc = useQueryClient()
  const [page, setPage] = useState(0)
  const { data, isLoading, isError, error, refetch } = useQuery({
    queryKey: ["releases"],
    queryFn: () => getApiReleases() as Promise<{
      arch: string
      archAuto: boolean
      activeTag: string | null
      currentBinary: string
      knownArchs: string[]
      releases: ReleaseEntry[]
    }>,
  })

  const invalidate = () => void qc.invalidateQueries({ queryKey: ["releases"] })

  if (isError) {
    return (
      <div className="flex flex-col items-center justify-center gap-2 py-16 text-muted-foreground">
        <span className="text-[12px] text-[var(--danger)]">
          Failed to load releases{error?.message ? `: ${error.message}` : ""}
        </span>
        <button
          onClick={() => void refetch()}
          className="flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
        >
          <RefreshCw className="size-3" />
          Retry
        </button>
      </div>
    )
  }

  if (isLoading || !data) {
    return (
      <div className="flex flex-col items-center justify-center gap-2 py-16 text-muted-foreground">
        <Loader2 className="size-5 animate-spin" />
        <span className="text-[12px]">Loading releases…</span>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <ArchSection
        arch={data.arch}
        archAuto={data.archAuto}
        knownArchs={data.knownArchs}
        onChanged={invalidate}
      />

      <div className="flex items-center justify-between">
        <h3 className="text-[11px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
          Releases
        </h3>
        <button
          onClick={() => void refetch()}
          className="flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
        >
          <RefreshCw className="size-3" />
          Refresh
        </button>
      </div>

      {data.releases.length === 0 ? (
        <p className="py-8 text-center text-[12px] text-muted-foreground/70">
          No releases found for architecture <strong>{data.arch}</strong>.
        </p>
      ) : (
        <PaginatedReleases
          releases={data.releases}
          page={page}
          setPage={setPage}
          onChanged={invalidate}
        />
      )}

      <p className="text-[10.5px] text-muted-foreground/60">
        Current binary: <code className="font-mono text-[10px]">{data.currentBinary}</code>
      </p>
    </div>
  )
}

// ── Architecture selector ─────────────────────────────────────────

function ArchSection({
  arch,
  archAuto,
  knownArchs,
  onChanged,
}: {
  arch: string
  archAuto: boolean
  knownArchs: string[]
  onChanged: () => void
}) {
  const [open, setOpen] = useState(false)
  const setArch = useMutation({
    mutationFn: async (v: { arch?: string; auto?: boolean }) => {
      await putApiReleasesArch({ body: v })
    },
    onSuccess: onChanged,
  })

  return (
    <div className="flex flex-col gap-2">
      <span className="text-[10.5px] font-semibold uppercase tracking-[0.07em] text-muted-foreground/80">
        Architecture
      </span>
      <div className="flex items-center gap-2">
        <div className="relative">
          <button
            onClick={() => setOpen((v) => !v)}
            className="flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-[13px] font-medium text-foreground/90 transition-colors hover:bg-muted/40 card-shadow"
          >
            <Package className="size-4 text-muted-foreground/70" />
            {arch}
            <ChevronDown className={cn("size-3.5 text-muted-foreground/60 transition-transform", open && "rotate-180")} />
          </button>
          {open && (
            <div className="absolute left-0 top-full z-10 mt-1 min-w-[200px] rounded-lg border border-border bg-card py-1 card-shadow">
              {knownArchs.map((a) => (
                <button
                  key={a}
                  onClick={() => { setArch.mutate({ arch: a }); setOpen(false) }}
                  className={cn(
                    "flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12.5px] transition-colors hover:bg-muted/50",
                    a === arch ? "font-medium text-[var(--interactive)]" : "text-foreground/80",
                  )}
                >
                  {a === arch && <Check className="size-3" strokeWidth={3} />}
                  <span className={a === arch ? "" : "pl-5"}>{a}</span>
                </button>
              ))}
            </div>
          )}
        </div>
        <button
          onClick={() => setArch.mutate({ auto: true })}
          disabled={setArch.isPending}
          className={cn(
            "flex items-center gap-1.5 rounded-lg border px-3 py-2 text-[12px] font-medium transition-all",
            archAuto
              ? "border-[var(--interactive)]/30 bg-[var(--interactive)]/[0.06] text-[var(--interactive)]"
              : "border-border bg-card text-foreground/75 hover:bg-muted/40 card-shadow",
          )}
        >
          {setArch.isPending ? <Loader2 className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}
          Auto-detect
        </button>
        {archAuto && (
          <span className="text-[10.5px] text-[var(--interactive)]">
            ✓ auto-detected
          </span>
        )}
      </div>
    </div>
  )
}

// ── Paginated release list ────────────────────────────────────────

function PaginatedReleases({
  releases,
  page,
  setPage,
  onChanged,
}: {
  releases: ReleaseEntry[]
  page: number
  setPage: (p: number) => void
  onChanged: () => void
}) {
  const totalPages = Math.ceil(releases.length / PAGE_SIZE)
  const safePage = Math.min(page, totalPages - 1)
  const slice = releases.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE)

  return (
    <div className="flex flex-col gap-2">
      {slice.map((r, i) => (
        <ReleaseCard
          key={r.tag}
          release={r}
          index={i}
          onChanged={onChanged}
        />
      ))}

      {totalPages > 1 && (
        <div className="flex items-center justify-center gap-3 pt-1">
          <button
            onClick={() => setPage(Math.max(0, safePage - 1))}
            disabled={safePage === 0}
            className="rounded-md px-2 py-1 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground disabled:opacity-30"
          >
            ← Prev
          </button>
          <span className="text-[11px] tabular-nums text-muted-foreground">
            {safePage + 1} / {totalPages}
          </span>
          <button
            onClick={() => setPage(Math.min(totalPages - 1, safePage + 1))}
            disabled={safePage >= totalPages - 1}
            className="rounded-md px-2 py-1 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground disabled:opacity-30"
          >
            Next →
          </button>
        </div>
      )}
    </div>
  )
}

// ── Release card ──────────────────────────────────────────────────

function ReleaseCard({
  release: r,
  index,
  onChanged,
}: {
  release: ReleaseEntry
  index: number
  onChanged: () => void
}) {
  const dl = useMutation({
    mutationFn: async () => { await postApiReleasesDownload({ body: { tag: r.tag } }) },
    onSuccess: onChanged,
  })
  const sel = useMutation({
    mutationFn: async () => { await putApiReleasesSelect({ body: { tag: r.tag } }) },
    onSuccess: onChanged,
  })
  const del = useMutation({
    mutationFn: async () => { await deleteApiReleasesByTag({ path: { tag: r.tag } }) },
    onSuccess: onChanged,
  })

  const busy = dl.isPending || sel.isPending || del.isPending

  return (
    <div
      style={{ animationDelay: `${index * 40}ms` }}
      className={cn(
        "opt-rise flex items-center gap-3 rounded-xl border px-3.5 py-3 card-shadow",
        r.selected
          ? "border-[var(--interactive)]/40 bg-[var(--interactive)]/[0.04]"
          : "border-border bg-card",
      )}
    >
      {/* Left: tag + meta */}
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-center gap-2">
          <span className="truncate font-mono text-[13px] font-medium text-foreground/90">
            {r.tag}
          </span>
          {r.isLatest && <Chip color="interactive" label="Latest" />}
          {r.local && <Chip color="ok" label="Downloaded" />}
          {r.selected && <Chip color="interactive" label="Active" />}
        </div>
        <div className="flex items-center gap-3 text-[11px] text-muted-foreground">
          {r.name !== r.tag && <span>{r.name}</span>}
          {r.publishedAt && <span>{formatDate(r.publishedAt)}</span>}
          {r.assetSize != null && <span>{formatSize(r.assetSize)}</span>}
          {r.local && r.binarySize != null && <span>binary {formatSize(r.binarySize)}</span>}
        </div>
      </div>

      {/* Right: actions */}
      <div className="flex shrink-0 items-center gap-1.5">
        {!r.local && r.assetUrl && (
          <ActionBtn
            icon={Download}
            label="Download"
            pending={dl.isPending}
            disabled={busy}
            onClick={() => dl.mutate()}
          />
        )}
        {!r.local && !r.assetUrl && (
          <span className="text-[10px] italic text-muted-foreground/50">
            no asset
          </span>
        )}
        {r.local && !r.selected && (
          <ActionBtn
            icon={Check}
            label="Use"
            pending={sel.isPending}
            disabled={busy}
            onClick={() => sel.mutate()}
            accent
          />
        )}
        {r.local && !r.selected && (
          <ActionBtn
            icon={Trash2}
            label="Delete"
            pending={del.isPending}
            disabled={busy}
            onClick={() => del.mutate()}
            danger
          />
        )}
      </div>
    </div>
  )
}

// ── Tiny helpers ──────────────────────────────────────────────────

function Chip({ color, label }: { color: "interactive" | "ok"; label: string }) {
  const cls = color === "ok"
    ? "bg-[var(--ok)]/12 text-[var(--ok)]"
    : "bg-[var(--interactive)]/12 text-[var(--interactive)]"
  return (
    <span className={cn("inline-flex shrink-0 rounded-full px-1.5 py-px text-[9.5px] font-semibold", cls)}>
      {label}
    </span>
  )
}

function ActionBtn({
  icon: Icon,
  label,
  pending,
  disabled,
  onClick,
  accent,
  danger,
}: {
  icon: typeof Download
  label: string
  pending: boolean
  disabled: boolean
  onClick: () => void
  accent?: boolean
  danger?: boolean
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={label}
      className={cn(
        "flex items-center gap-1 rounded-lg border px-2.5 py-1.5 text-[11px] font-medium transition-all disabled:opacity-50",
        accent && "border-[var(--interactive)]/30 bg-[var(--interactive)]/[0.06] text-[var(--interactive)] hover:bg-[var(--interactive)]/[0.12]",
        danger && "border-[var(--danger)]/30 bg-[var(--danger)]/[0.06] text-[var(--danger)] hover:bg-[var(--danger)]/[0.12]",
        !accent && !danger && "border-border bg-card text-foreground/75 hover:bg-muted/40",
      )}
    >
      {pending ? <Loader2 className="size-3.5 animate-spin" /> : <Icon className="size-3.5" />}
      {label}
    </button>
  )
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" })
  } catch {
    return iso
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
