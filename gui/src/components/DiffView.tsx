import { memo } from 'react'

/** Classify one diff line for the add/remove/hunk styling. */
function lineClass(line: string): string {
  if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('@@')) return 'h'
  if (line.startsWith('+')) return 'a'
  if (line.startsWith('-')) return 'd'
  return ''
}

/** Display-oriented diff rendered with add/remove line colouring. */
export const DiffView = memo(function DiffView({ diff }: { diff: string }) {
  const lines = diff.length > 0 ? diff.split('\n') : []

  return (
    <div className="diff">
      {lines.map((line, index) => (
        <div className={lineClass(line)} key={index}>
          {line}
        </div>
      ))}
    </div>
  )
})
