import { useState } from "react"
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Loader2, Mail, Trash2 } from "lucide-react"
import type { ItSmsMessage, ItSmsStatus } from "@/lib/api/generated/types.gen"
import {
  apiErrorMessage,
  deleteItSms,
  fetchItNetwork,
  fetchItSms,
  markItSmsRead,
  sendItSms,
} from "@/lib/api"
import { IT_NETWORK_KEY, POLL_MS } from "@/lib/api/it/network"
import type { SmsDraft, SmsTone } from "@/lib/api/it/sms"
import {
  EMPTY_SMS_DRAFT,
  IT_SMS_KEY,
  SMS_MAX_BODY,
  SMS_PAGE,
  SMS_POLL_MS,
  flattenSmsPages,
  nextSmsCursor,
  sendProblem,
  smsBody,
  smsBodyLength,
  smsView,
  unreadLabel,
} from "@/lib/api/it/sms"
import { cn } from "@/lib/utils"
import { SectionLabel, TextField } from "./ItPane"

/**
 * SMS on the box's own SIM — read the archive, send, mark read, remove.
 *
 * A sibling of {@link ItNetworkPane}, mounted next to it by `ConfigPanes`, so it
 * inherits the same `can_manage_it` gate: the client-side gating is cosmetic and
 * the backend answers 403 to anyone else regardless.
 *
 * **The panel does not exist on a box that cannot do SMS.** Not disabled, not
 * greyed — absent. `status.sms` is null when there is no 5G module or no
 * `mmcli`, and that null is the whole promise "SMS only on 5G Photonicats"; a
 * disabled panel would advertise a feature the hardware will never grow. It is
 * read from the SAME `GET /api/it/network` query the uplink pane already polls
 * (same key, same interval), so mounting this pane costs no extra request and
 * the two panes cannot disagree about what the box is.
 *
 * Everything with no styling in it — the poll interval, the cursor arithmetic,
 * the validation mirror, the per-message view model — lives in
 * `@/lib/api/it/sms`, so this file and its mobile twin differ only in Tailwind
 * classes (C8).
 */
export function ItSmsPane() {
  const { data } = useQuery({
    queryKey: IT_NETWORK_KEY,
    queryFn: fetchItNetwork,
    refetchInterval: POLL_MS,
  })

  // Nothing at all until the box has SAID it can do SMS. Rendering a frame
  // while the first read is in flight would put an SMS panel in the DOM of a
  // modem-less box for as long as the request takes, which is exactly what
  // `e2e/sms.spec.ts` asserts never happens.
  const sms = data?.status.sms ?? null
  if (sms?.available !== true) return null

  return (
    <section className="flex flex-col gap-2">
      <SectionLabel label="SMS" hint="Messages on this box's SIM" />
      <SmsCard status={sms} />
    </section>
  )
}

/** The panel proper, once the box has confirmed it has a modem. */
function SmsCard({ status }: { status: ItSmsStatus }) {
  const badge = unreadLabel(status.unread)
  return (
    <div className="flex flex-col gap-3 rounded-xl border border-border bg-card px-3.5 py-3">
      <div className="flex items-center gap-2">
        <Mail className="size-3.5 text-muted-foreground" />
        <span className="text-[12px] font-medium text-foreground/90">Inbox</span>
        {badge !== null && (
          <span className="rounded-full bg-(--interactive)/15 px-2 py-0.5 text-[10.5px] font-semibold text-(--interactive)">
            {badge}
          </span>
        )}
      </div>
      <Inbox />
      <ComposeForm />
    </div>
  )
}

// ── Inbox ────────────────────────────────────────────────────────────

/**
 * The archive, newest first, one page at a time.
 *
 * `useInfiniteQuery` rather than a growing `limit`: the route paginates on
 * `?before=<id>`, so each page is a stable slice that a message arriving
 * meanwhile cannot shift — a growing limit would re-fetch the whole visible
 * history on every "Load older" and could show a row twice.
 */
function Inbox() {
  const list = useInfiniteQuery({
    queryKey: IT_SMS_KEY,
    queryFn: ({ pageParam }) => fetchItSms(pageParam, SMS_PAGE),
    initialPageParam: null as number | null,
    getNextPageParam: nextSmsCursor,
    refetchInterval: SMS_POLL_MS,
  })

  if (list.isError) {
    return (
      <p className="text-[11px] text-(--danger)">
        {apiErrorMessage(list.error, "Could not read this box's messages.")}
      </p>
    )
  }
  if (list.data === undefined) {
    return (
      <div className="flex items-center gap-2 text-[12px] text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" /> Loading…
      </div>
    )
  }

  const messages = flattenSmsPages(list.data.pages)
  if (messages.length === 0) {
    return <p className="text-[11px] text-muted-foreground">No messages on this SIM yet.</p>
  }

  return (
    <div className="flex flex-col gap-1.5">
      {messages.map((message) => (
        <MessageRow key={message.id} message={message} />
      ))}
      {list.hasNextPage && (
        <button
          type="button"
          disabled={list.isFetchingNextPage}
          onClick={() => void list.fetchNextPage()}
          className="self-start rounded-md px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground disabled:opacity-50"
        >
          {list.isFetchingNextPage ? "Loading…" : "Load older"}
        </button>
      )}
    </div>
  )
}

/** Delivery colours, the only place tone becomes pixels. */
const TONE: Record<SmsTone, string> = {
  muted: "text-muted-foreground/70",
  warn: "text-(--warn)",
  ok: "text-(--ok)",
  danger: "text-(--danger)",
}

/**
 * One message.
 *
 * Inbound and outbound are told apart three ways at once — the side the card
 * sits on, its ground colour, and the "From/To" label — because a single cue is
 * one theme change away from being invisible, and on this panel confusing the
 * two means answering a message the box itself sent.
 *
 * Opening an unread inbound message is what marks it read. NOT mounting: the
 * badge would then be zeroed by anyone who merely walked past Settings, and a
 * badge that lies is worse than no badge.
 */
function MessageRow({ message }: { message: ItSmsMessage }) {
  const qc = useQueryClient()
  const [open, setOpen] = useState(false)
  const view = smsView(message)

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: IT_SMS_KEY })
    // The unread badge lives on the OTHER query (`status.sms`), so both have to
    // be refreshed or the two halves of this panel disagree.
    void qc.invalidateQueries({ queryKey: IT_NETWORK_KEY })
  }

  const read = useMutation({ mutationFn: () => markItSmsRead(message.id), onSuccess: invalidate })
  const drop = useMutation({ mutationFn: () => deleteItSms(message.id), onSuccess: invalidate })

  const toggle = () => {
    if (!open && view.unread) read.mutate()
    setOpen((previous) => !previous)
  }

  return (
    <div
      className={cn(
        "flex flex-col gap-1 rounded-md border px-2.5 py-2",
        view.inbound ? "border-border bg-muted/40" : "ml-6 border-(--interactive)/30 bg-card",
      )}
    >
      <button type="button" onClick={toggle} className="flex flex-col gap-1 text-left">
        <span className="flex items-baseline gap-2">
          <span
            className={cn(
              "text-[11px] tracking-[0.04em] text-muted-foreground/70 uppercase",
              view.unread && "font-semibold text-foreground/90",
            )}
          >
            {view.inbound ? "From" : "To"}
          </span>
          <span className="font-mono text-[12px] text-foreground/90">{view.peer}</span>
          {view.unread && <span className="size-1.5 rounded-full bg-(--interactive)" />}
          <span className="ml-auto text-[11px] text-muted-foreground/70" title={view.exact}>
            {view.when}
          </span>
        </span>
        <span
          className={cn(
            "text-[12px] text-foreground/85",
            !open && "line-clamp-2",
            view.unread && "font-medium",
          )}
        >
          {view.body}
        </span>
      </button>

      <div className="flex items-center gap-2">
        <span className={cn("text-[11px]", TONE[view.tone])}>{view.delivery}</span>
        {view.sentBy !== null && (
          <span className="text-[11px] text-muted-foreground/70">{view.sentBy}</span>
        )}
        {open && (
          <button
            type="button"
            disabled={drop.isPending}
            onClick={() => drop.mutate()}
            aria-label="Remove this message from the list"
            className="ml-auto flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] text-muted-foreground transition-colors hover:bg-(--danger)/10 hover:text-(--danger) disabled:opacity-50"
          >
            <Trash2 className="size-3" />
            Remove
          </button>
        )}
      </div>

      {/* The modem's own words. Never folded into "Failed": the reason is the
          only half an operator can act on. */}
      {view.error !== null && <span className="text-[11px] text-(--danger)">{view.error}</span>}
      {drop.isError && (
        <span className="text-[11px] text-(--danger)">
          {apiErrorMessage(drop.error, "Could not remove this message")}
        </span>
      )}
    </div>
  )
}

// ── Compose ──────────────────────────────────────────────────────────

/**
 * Send one message.
 *
 * Every rule that decides whether the button is live lives in
 * `@/lib/api/it/sms` and mirrors `sms/mod.rs::validate` exactly. What it
 * deliberately does not mirror is the rate limit (10/h per operator, 50/day for
 * the box) or the modem's refusal: neither is knowable from here, so a `429` and
 * a `502` are printed in the server's own words instead of being swallowed into
 * "something went wrong". Sending spends the vendor's data plan — an operator
 * who has hit the ceiling needs to read the ceiling.
 */
function ComposeForm() {
  const qc = useQueryClient()
  const [draft, setDraft] = useState<SmsDraft>(EMPTY_SMS_DRAFT)

  const send = useMutation({
    mutationFn: () => sendItSms(smsBody(draft)),
    onSuccess: () => {
      // The send wrote a row before the modem was touched, so the archive is
      // already ahead of the cache; snap the form back and re-read both halves.
      setDraft(EMPTY_SMS_DRAFT)
      void qc.invalidateQueries({ queryKey: IT_SMS_KEY })
      void qc.invalidateQueries({ queryKey: IT_NETWORK_KEY })
    },
  })

  /** Edit one field, clearing the send banner on the first keystroke so a stale
   *  "Sent" never sits under a form that no longer matches it. */
  const edit =
    <K extends keyof SmsDraft>(field: K) =>
    (value: SmsDraft[K]) => {
      send.reset()
      setDraft((previous) => ({ ...previous, [field]: value }))
    }

  const problem = sendProblem(draft)
  const used = smsBodyLength(draft.body)

  return (
    <form
      className="flex flex-col gap-2.5 border-t border-border pt-3"
      onSubmit={(event) => {
        event.preventDefault()
        if (problem === null && !send.isPending) send.mutate()
      }}
    >
      <TextField
        label="Send to"
        hint="6–15 digits, optionally with +"
        value={draft.to}
        onChange={edit("to")}
        placeholder="+33612345678"
        inputMode="numeric"
      />
      <label className="flex flex-col gap-1">
        <span className="flex items-baseline gap-2 text-[12px] font-medium text-foreground/90">
          Message
          <span
            className={cn(
              "text-[11px] font-normal text-muted-foreground/60",
              used > SMS_MAX_BODY && "text-(--danger)",
            )}
          >
            {used} / {SMS_MAX_BODY}
          </span>
        </span>
        <textarea
          value={draft.body}
          onChange={(event) => edit("body")(event.target.value)}
          disabled={send.isPending}
          rows={3}
          placeholder="Type your message"
          className="w-full resize-y rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[12px] text-foreground placeholder:text-muted-foreground/50 focus:ring-1 focus:ring-(--interactive) focus:outline-none disabled:opacity-50"
        />
      </label>

      <div className="flex items-center gap-2">
        <button
          type="submit"
          disabled={problem !== null || send.isPending}
          className="flex items-center gap-1.5 rounded-md bg-(--interactive) px-3 py-1.5 text-[12px] font-medium text-(--primary-foreground) transition-all hover:brightness-105 disabled:opacity-50"
        >
          {send.isPending && <Loader2 className="size-3.5 animate-spin" />}
          Send
        </button>
        {problem !== null && <span className="text-[11px] text-muted-foreground">{problem}</span>}
        {send.isSuccess && <span className="text-[11px] text-(--ok)">Sent</span>}
        {send.isError && (
          <span className="text-[11px] text-(--danger)">
            {apiErrorMessage(send.error, "Could not send the message")}
          </span>
        )}
      </div>
      <p className="text-[11px] text-muted-foreground">
        Messages are sent on this box's own SIM and charged to its data plan, so every send is rate
        limited and recorded with the operator who ordered it.
      </p>
    </form>
  )
}
