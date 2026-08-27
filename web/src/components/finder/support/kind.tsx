import type { FinderKind } from "@/lib/types"

/** Extension helper for `<FileIcon ext>` — bare uppercase ext from a filename. */
export function extOf(name: string): string | undefined {
  const i = name.lastIndexOf(".")
  return i > 0 ? name.slice(i + 1) : undefined
}

/**
 * Infer a {@link FinderKind} from a filename's extension — the client mirror of
 * the backend's `infer_kind` (finder/support.rs). Used to synthesize a
 * {@link FinderNode} for a file referenced only by path (e.g. a chat
 * `file-upload` attachment chip) so the shared Quick Look preview routes it to
 * the right viewer. Kept in lockstep with the backend table; an unknown
 * extension falls back to `"binary"`.
 */
const KIND_BY_EXT: Record<string, FinderKind> = {
  rs: "code",
  py: "code",
  js: "code",
  ts: "code",
  tsx: "code",
  jsx: "code",
  go: "code",
  c: "code",
  cpp: "code",
  h: "code",
  hpp: "code",
  java: "code",
  rb: "code",
  sh: "code",
  bash: "code",
  zsh: "code",
  lua: "code",
  zig: "code",
  swift: "code",
  kt: "code",
  scala: "code",
  ex: "code",
  exs: "code",
  erl: "code",
  hs: "code",
  ml: "code",
  css: "code",
  scss: "code",
  html: "code",
  sql: "code",
  r: "code",
  pl: "code",
  php: "code",
  cs: "code",
  fs: "code",
  vue: "code",
  svelte: "code",
  dart: "code",
  nim: "code",
  v: "code",
  wasm: "code",
  md: "markdown",
  mdx: "markdown",
  json: "json",
  jsonl: "json",
  json5: "json",
  pdf: "pdf",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  svg: "image",
  webp: "image",
  bmp: "image",
  ico: "image",
  tiff: "image",
  heic: "image",
  csv: "sheet",
  xlsx: "sheet",
  xls: "sheet",
  ods: "sheet",
  tsv: "sheet",
  pptx: "slides",
  ppt: "slides",
  odp: "slides",
  zip: "archive",
  tar: "archive",
  gz: "archive",
  bz2: "archive",
  xz: "archive",
  "7z": "archive",
  rar: "archive",
  zst: "archive",
  mp3: "audio",
  wav: "audio",
  flac: "audio",
  m4a: "audio",
  ogg: "audio",
  aac: "audio",
  wma: "audio",
  mp4: "video",
  mov: "video",
  avi: "video",
  mkv: "video",
  webm: "video",
  wmv: "video",
  flv: "video",
  txt: "doc",
  log: "doc",
  yml: "doc",
  yaml: "doc",
  toml: "doc",
  cfg: "doc",
  ini: "doc",
  env: "doc",
  conf: "doc",
}

export function kindOf(name: string): FinderKind {
  const ext = (name.split(".").pop() ?? "").toLowerCase()
  return KIND_BY_EXT[ext] ?? "binary"
}
