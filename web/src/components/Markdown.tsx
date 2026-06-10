import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { CodeBlock } from './CodeBlock'

/** Rendu markdown de la conversation, blocs de code via Shiki. */
export function Markdown({ children }: { children: string }) {
  return (
    <div className="prose-nestor">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          pre({ children }) {
            return <>{children}</>
          },
          code(props) {
            const { className, children } = props
            const match = /language-(\w+)/.exec(className ?? '')
            const text = String(children).replace(/\n$/, '')
            if (match || text.includes('\n')) {
              return <CodeBlock code={text} lang={match?.[1] ?? 'text'} />
            }
            return <code className={className}>{children}</code>
          },
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  )
}
