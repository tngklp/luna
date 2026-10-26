import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

/** One row in a context menu. */
export interface MenuItem {
  label: string
  onSelect: () => void
  /** Rendered in the danger colour. */
  danger?: boolean
  disabled?: boolean
  /** Draw a divider above this item. */
  separated?: boolean
}

interface ContextMenuProps {
  /** Viewport coordinates of the click that opened the menu. */
  x: number
  y: number
  items: MenuItem[]
  onClose: () => void
}

/** Space kept between the menu and the window edge. */
const EDGE = 8

/**
 * A right-click menu.
 *
 * Rendered into `document.body` rather than beside its trigger: the session
 * list scrolls and clips, so a menu positioned inside a row would be cut off at
 * the list's edge. The position is measured before paint and nudged back inside
 * the viewport, so a click near the bottom-right corner still shows the whole
 * menu.
 */
export function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement | null>(null)
  const [at, setAt] = useState({ left: x, top: y })

  useLayoutEffect(() => {
    const menu = ref.current
    if (!menu) return
    const { width, height } = menu.getBoundingClientRect()
    setAt({
      left: Math.max(EDGE, Math.min(x, window.innerWidth - width - EDGE)),
      top: Math.max(EDGE, Math.min(y, window.innerHeight - height - EDGE))
    })
  }, [x, y, items.length])

  useEffect(() => {
    const onPointerDown = (event: PointerEvent): void => {
      const target = event.target as HTMLElement | null
      // The row's own "⋯" button toggles the menu; closing here would make a
      // second click reopen it instead of dismissing it.
      if (target?.closest('.srow__more')) return
      if (!ref.current?.contains(target as Node)) onClose()
    }
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') onClose()
    }
    // A menu anchored to a point goes stale as soon as the page moves under it.
    const onMove = (): void => onClose()

    window.addEventListener('pointerdown', onPointerDown, true)
    window.addEventListener('keydown', onKey)
    window.addEventListener('resize', onMove)
    window.addEventListener('wheel', onMove, { passive: true })
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true)
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('resize', onMove)
      window.removeEventListener('wheel', onMove)
    }
  }, [onClose])

  return createPortal(
    <div
      className="ctxmenu"
      role="menu"
      ref={ref}
      style={{ left: at.left, top: at.top }}
      onContextMenu={(event) => event.preventDefault()}
    >
      {items.map((item) => (
        <button
          key={item.label}
          type="button"
          role="menuitem"
          disabled={item.disabled}
          className={`ctxmenu__item${item.danger ? ' bad' : ''}${item.separated ? ' sep' : ''}`}
          onClick={() => {
            onClose()
            item.onSelect()
          }}
        >
          {item.label}
        </button>
      ))}
    </div>,
    document.body
  )
}
