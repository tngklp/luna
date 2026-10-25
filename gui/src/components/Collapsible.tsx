import type { ReactNode } from 'react'

/**
 * Animates its children open and closed with a `grid-template-rows`
 * transition, so both directions are animated (unlike `<details>`).
 */
export function Collapsible({ open, children }: { open: boolean; children: ReactNode }) {
  return (
    <div className={`collapse${open ? ' open' : ''}`}>
      <div className="collapse__inner">{children}</div>
    </div>
  )
}
