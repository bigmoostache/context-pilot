import { useEffect, useState } from 'react'
import type { HighlighterCore } from 'shiki'

let highlighterPromise: Promise<HighlighterCore> | null = null

/** Shiki paresseux : moteur JS (pas d'oniguruma WASM) + thème unique. */
async function getHighlighter(): Promise<HighlighterCore> {
  if (!highlighterPromise) {
    highlighterPromise = (async () => {
      const { createHighlighterCore } = await import('shiki/core')
      const { createJavaScriptRegexEngine } = await import('shiki/engine/javascript')
      return createHighlighterCore({
        themes: [import('@shikijs/themes/vesper')],
        langs: [
          import('@shikijs/langs/rust'),
          import('@shikijs/langs/typescript'),
          import('@shikijs/langs/tsx'),
          import('@shikijs/langs/javascript'),
          import('@shikijs/langs/python'),
          import('@shikijs/langs/go'),
          import('@shikijs/langs/bash'),
          import('@shikijs/langs/json'),
          import('@shikijs/langs/yaml'),
          import('@shikijs/langs/toml'),
          import('@shikijs/langs/markdown'),
          import('@shikijs/langs/html'),
          import('@shikijs/langs/css'),
          import('@shikijs/langs/sql'),
          import('@shikijs/langs/c'),
          import('@shikijs/langs/cpp'),
          import('@shikijs/langs/java'),
        ],
        engine: createJavaScriptRegexEngine({ forgiving: true }),
      })
    })()
  }
  return highlighterPromise
}

/** Bloc de code coloré côté client (fallback : <pre> brut pendant le chargement). */
export function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const [html, setHtml] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    getHighlighter().then((hl) => {
      if (!alive) return
      const loaded = hl.getLoadedLanguages()
      const language = loaded.includes(lang) ? lang : 'text'
      try {
        setHtml(hl.codeToHtml(code, { lang: language, theme: 'vesper' }))
      } catch {
        setHtml(null)
      }
    })
    return () => {
      alive = false
    }
  }, [code, lang])

  if (html) {
    return (
      <div
        className="overflow-x-auto rounded-lg border border-coal-700 bg-coal-900/80 p-3 font-mono text-[0.8rem] leading-relaxed [&_pre]:!m-0"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    )
  }
  return (
    <pre className="overflow-x-auto rounded-lg border border-coal-700 bg-coal-900/80 p-3 font-mono text-[0.8rem] leading-relaxed">
      <code>{code}</code>
    </pre>
  )
}
