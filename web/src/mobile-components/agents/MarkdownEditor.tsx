import { useCallback, useEffect, useRef, useState } from "react"
import {
  Bold,
  Code,
  Code2,
  Heading1,
  Heading2,
  Italic,
  Link2,
  List,
  ListOrdered,
  Quote,
  Redo2,
  Strikethrough,
  Table,
  Undo2,
} from "lucide-react"
import type { ReactNode } from "react"
import { useEditor, EditorContent } from "@tiptap/react"
import StarterKit from "@tiptap/starter-kit"
import LinkExtension from "@tiptap/extension-link"
import Placeholder from "@tiptap/extension-placeholder"
import TaskList from "@tiptap/extension-task-list"
import TaskItem from "@tiptap/extension-task-item"
import { Table as TableExtension } from "@tiptap/extension-table"
import TableRow from "@tiptap/extension-table-row"
import TableHeader from "@tiptap/extension-table-header"
import TableCell from "@tiptap/extension-table-cell"
import Image from "@tiptap/extension-image"
import { Markdown as MarkdownExtension } from "tiptap-markdown"
import { cn } from "@/lib/utils"
import { TableGridPicker, TableContextBar } from "./tableControls"

/** Extract tiptap-markdown storage safely (avoids banned double-assertion). */
function getMdStorage(storage: unknown): { getMarkdown: () => string } | undefined {
  const s = storage as Record<string, unknown>
  const md = s["markdown"]
  return md && typeof md === "object" && "getMarkdown" in md
    ? (md as { getMarkdown: () => string })
    : undefined
}

/**
 * WYSIWYG markdown editor — mobile twin of `components/agents/MarkdownEditor`.
 *
 * Same TipTap/ProseMirror engine, same extension set, same markdown round-trip.
 * The fork is touch sizing: the toolbar buttons grow to 36px tap targets (from
 * the desktop 28px), the toolbar scrolls horizontally (`overflow-x-auto`,
 * `no-scrollbar`) since a phone can't fit ~15 buttons across, the editor body is
 * 16px text (defeats iOS focus-zoom), and press feedback swaps `hover:` for
 * `active:`. The table grid picker + context bar are shared (./tableControls
 * mobile twin).
 */
export function MarkdownEditor({
  initialMarkdown,
  placeholder,
  className,
  onChange,
  toolbarExtra,
}: {
  initialMarkdown: string
  placeholder?: string
  className?: string
  onChange?: (markdown: string) => void
  toolbarExtra?: ReactNode
}) {
  // Keep latest onChange in a ref so useEditor binds once on mount.
  const onChangeRef = useRef(onChange)
  useEffect(() => {
    onChangeRef.current = onChange
  }, [onChange])

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
      }),
      LinkExtension.configure({
        openOnClick: false,
        HTMLAttributes: { class: "text-(--interactive) underline underline-offset-2" },
      }),
      Placeholder.configure({
        placeholder: placeholder ?? "",
      }),
      TaskList,
      TaskItem.configure({ nested: true }),
      TableExtension.configure({ resizable: true }),
      TableRow,
      TableHeader,
      TableCell,
      Image.configure({ inline: false, allowBase64: true }),
      MarkdownExtension.configure({
        html: false,
        transformPastedText: true,
        transformCopiedText: true,
      }),
    ],
    content: initialMarkdown,
    // Re-render on every transaction so the table context bar appears instantly
    // on tap (selection-only transactions are otherwise skipped).
    shouldRerenderOnTransaction: true,
    onUpdate: ({ editor: ed }) => {
      const mdExt = getMdStorage(ed.storage)
      if (mdExt) onChangeRef.current?.(mdExt.getMarkdown())
    },
    editorProps: {
      attributes: {
        class: cn(
          // 16px base body text — defeats iOS focus-zoom (desktop uses 13.5px).
          "prose-editor min-h-full p-4 text-[16px] leading-relaxed text-foreground/90 outline-none",
          "[&_h1]:mt-3 [&_h1]:mb-2 [&_h1]:text-[20px] [&_h1]:font-bold [&_h1]:tracking-tight [&_h1]:text-foreground",
          "[&_h2]:mt-3 [&_h2]:mb-1.5 [&_h2]:text-[17px] [&_h2]:font-semibold [&_h2]:text-foreground",
          "[&_h3]:mt-2.5 [&_h3]:mb-1 [&_h3]:text-[15px] [&_h3]:leading-snug [&_h3]:font-semibold",
          "[&_p]:my-1.5",
          "[&_li]:my-0.5 [&_ol]:my-1.5 [&_ol]:list-decimal [&_ol]:pl-5 [&_ul]:my-1.5 [&_ul]:list-disc [&_ul]:pl-5",
          "[&_blockquote]:my-2 [&_blockquote]:border-l-2 [&_blockquote]:border-(--signal)/60 [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground",
          "[&_pre]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:bg-(--surface-2)/60 [&_pre]:px-3 [&_pre]:py-2 [&_pre]:font-mono [&_pre]:text-[13px]",
          "[&_code]:rounded-sm [&_code]:bg-muted [&_code]:px-1 [&_code]:font-mono [&_code]:text-[13.5px] [&_code]:text-(--signal)",
          "[&_a]:text-(--interactive) [&_a]:underline [&_a]:underline-offset-2",
          "[&_strong]:font-semibold [&_strong]:text-foreground",
          "[&_hr]:my-3 [&_hr]:border-t [&_hr]:border-border",
          // Tables
          "[&_table]:my-2 [&_table]:w-full [&_table]:border-collapse",
          "[&_th]:border [&_th]:border-border [&_th]:bg-muted/40 [&_th]:px-2.5 [&_th]:py-1.5 [&_th]:text-left [&_th]:text-[13.5px] [&_th]:font-semibold",
          "[&_td]:border [&_td]:border-border [&_td]:px-2.5 [&_td]:py-1.5 [&_td]:text-[13.5px]",
          // Task lists
          "[&_ul[data-type='taskList']]:list-none [&_ul[data-type='taskList']]:pl-0",
          "[&_ul[data-type='taskList']_li]:flex [&_ul[data-type='taskList']_li]:items-start [&_ul[data-type='taskList']_li]:gap-2",
          "[&_ul[data-type='taskList']_label]:mt-px",
          // Images
          "[&_img]:my-2 [&_img]:max-w-full [&_img]:rounded-lg",
        ),
        role: "textbox",
        "aria-multiline": "true",
        "aria-label": "Markdown editor",
        tabindex: "0",
        spellcheck: "false",
      },
    },
  })

  const inTable = editor.isActive("table")

  return (
    <div
      className={cn(
        "flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-border bg-card",
        className,
      )}
    >
      <Toolbar editor={editor} extra={toolbarExtra} />
      {inTable && <TableContextBar editor={editor} />}
      <div className="relative min-h-0 flex-1 overflow-y-auto">
        <EditorContent editor={editor} className="min-h-full" />
      </div>
    </div>
  )
}

/**
 * Single merged toolbar — horizontally scrollable on mobile (a phone can't fit
 * every formatting button across), formatting left, optional extra right.
 */
function Toolbar({
  editor,
  extra,
}: {
  editor: NonNullable<ReturnType<typeof useEditor>>
  extra?: ReactNode
}) {
  const [showGrid, setShowGrid] = useState(false)
  const toggleGrid = useCallback(() => setShowGrid((v) => !v), [])
  const closeGrid = useCallback(() => setShowGrid(false), [])

  const addLink = () => {
    const url = window.prompt("Link URL", "https://")
    if (url) {
      editor.chain().focus().extendMarkRange("link").setLink({ href: url }).run()
    }
  }

  return (
    <div className="no-scrollbar flex h-11 shrink-0 items-center gap-0.5 overflow-x-auto border-b border-border bg-muted/40 px-2">
      <Tool
        icon={Bold}
        title="Bold"
        on={editor.isActive("bold")}
        onClick={() => editor.chain().focus().toggleBold().run()}
      />
      <Tool
        icon={Italic}
        title="Italic"
        on={editor.isActive("italic")}
        onClick={() => editor.chain().focus().toggleItalic().run()}
      />
      <Tool
        icon={Strikethrough}
        title="Strikethrough"
        on={editor.isActive("strike")}
        onClick={() => editor.chain().focus().toggleStrike().run()}
      />
      <Tool
        icon={Code}
        title="Inline code"
        on={editor.isActive("code")}
        onClick={() => editor.chain().focus().toggleCode().run()}
      />
      <Sep />
      <Tool
        icon={Heading1}
        title="Heading 1"
        on={editor.isActive("heading", { level: 1 })}
        onClick={() => editor.chain().focus().toggleHeading({ level: 1 }).run()}
      />
      <Tool
        icon={Heading2}
        title="Heading 2"
        on={editor.isActive("heading", { level: 2 })}
        onClick={() => editor.chain().focus().toggleHeading({ level: 2 }).run()}
      />
      <Tool
        icon={Quote}
        title="Quote"
        on={editor.isActive("blockquote")}
        onClick={() => editor.chain().focus().toggleBlockquote().run()}
      />
      <Tool
        icon={Code2}
        title="Code block"
        on={editor.isActive("codeBlock")}
        onClick={() => editor.chain().focus().toggleCodeBlock().run()}
      />
      <Sep />
      <Tool
        icon={List}
        title="Bulleted list"
        on={editor.isActive("bulletList")}
        onClick={() => editor.chain().focus().toggleBulletList().run()}
      />
      <Tool
        icon={ListOrdered}
        title="Numbered list"
        on={editor.isActive("orderedList")}
        onClick={() => editor.chain().focus().toggleOrderedList().run()}
      />
      <Sep />
      <Tool icon={Link2} title="Link" on={editor.isActive("link")} onClick={addLink} />

      {/* Table button with grid picker popover */}
      <div className="relative">
        <Tool icon={Table} title="Insert table" onClick={toggleGrid} on={showGrid} />
        {showGrid && (
          <TableGridPicker
            onSelect={(rows, cols) => {
              editor.chain().focus().insertTable({ rows, cols, withHeaderRow: true }).run()
            }}
            onClose={closeGrid}
          />
        )}
      </div>

      <Sep />
      <Tool
        icon={Undo2}
        title="Undo"
        onClick={() => editor.chain().focus().undo().run()}
        disabled={!editor.can().undo()}
      />
      <Tool
        icon={Redo2}
        title="Redo"
        onClick={() => editor.chain().focus().redo().run()}
        disabled={!editor.can().redo()}
      />
      {extra != null && (
        <>
          <span className="flex-1" />
          {extra}
        </>
      )}
    </div>
  )
}

// ── Shared button primitives ───────────────────────────────────────

function Tool({
  icon: Icon,
  title,
  on,
  onClick,
  disabled,
}: {
  icon: typeof Bold
  title: string
  on?: boolean
  onClick: () => void
  disabled?: boolean
}) {
  return (
    <button
      type="button"
      title={title}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "flex size-9 shrink-0 items-center justify-center rounded-md transition-colors",
        disabled && "opacity-30",
        on
          ? "bg-(--signal)/15 text-(--signal)"
          : "text-muted-foreground/80 active:bg-muted/70 active:text-foreground",
      )}
    >
      <Icon className="size-[17px]" />
    </button>
  )
}

function Sep() {
  return <span className="mx-0.5 h-4 w-px shrink-0 bg-border" />
}
