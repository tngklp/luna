import { useEffect, useLayoutEffect, useRef, useState, type ReactElement } from 'react'

export interface SelectOption<T extends string> {
  value: T
  label: string
  /** Colours rendered as a small swatch before the label. */
  swatch?: string[]
}

interface SelectProps<T extends string> {
  value: T
  options: SelectOption<T>[]
  onChange: (value: T) => void
  disabled?: boolean
  title?: string
  /** Open the menu upward. Use where the control sits near the bottom. */
  direction?: 'up' | 'down'
  /** Stretch to the width of the container. */
  block?: boolean
  /** Hide the dropdown arrow. */
  hideCaret?: boolean
  placeholder?: string
}

/** Horizontal breathing room kept between a menu and its clipping container. */
const EDGE_GAP = 8

/**
 * Nearest ancestor that clips its children, which is what a dropdown can
 * overflow. Falls back to the viewport.
 */
function clippingBounds(element: HTMLElement): { left: number; right: number } {
  let left = EDGE_GAP
  let right = window.innerWidth - EDGE_GAP

  for (let node = element.parentElement; node; node = node.parentElement) {
    const style = getComputedStyle(node)
    if (/(auto|scroll|hidden|clip)/.test(`${style.overflowX}${style.overflowY}`)) {
      const rect = node.getBoundingClientRect()
      left = Math.max(left, rect.left + EDGE_GAP)
      right = Math.min(right, rect.right - EDGE_GAP)
    }
  }

  return { left, right }
}

/** Themed dropdown that replaces the native `select`. */
export function Select<T extends string>({
  value,
  options,
  onChange,
  disabled,
  title,
  direction = 'down',
  block,
  hideCaret,
  placeholder
}: SelectProps<T>) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (!open) return

    const onPointerDown = (event: globalThis.PointerEvent): void => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false)
    }
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') setOpen(false)
    }

    window.addEventListener('pointerdown', onPointerDown)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('pointerdown', onPointerDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [open])

  /**
   * Keep the menu inside the panel it belongs to. This runs before paint and
   * writes the transform straight onto the node, so nothing flickers.
   */
  useLayoutEffect(() => {
    const menu = menuRef.current
    const root = rootRef.current
    if (!open || !menu || !root) return

    menu.style.transform = ''

    const bounds = clippingBounds(root)
    const trigger = root.getBoundingClientRect()
    const width = menu.getBoundingClientRect().width

    let shift = 0
    if (trigger.left + width > bounds.right) shift = bounds.right - (trigger.left + width)
    if (trigger.left + shift < bounds.left) shift = bounds.left - trigger.left

    if (shift !== 0) menu.style.transform = `translateX(${shift}px)`
  }, [open, options.length])

  const current = options.find((option) => option.value === value)
  /** Widest label, used to size the closed control to its longest option. */
  const widest = options.reduce(
    (longest, option) => (option.label.length > longest.length ? option.label : longest),
    ''
  )

  const renderSwatch = (colors: string[]): ReactElement => (
    <span className="select__swatch" aria-hidden="true">
      {colors.map((color) => (
        <i key={color} style={{ background: color }} />
      ))}
    </span>
  )

  return (
    <div className={`select${block ? ' block' : ''}`} ref={rootRef}>
      {/* Invisible copy of the longest option so the control is never wider
          than it needs to be, and never clips its widest choice. */}
      {block ? null : (
        <span className="pick select__sizer" aria-hidden="true">
          {current?.swatch ? renderSwatch(current.swatch) : null}
          <span className="select__label">{widest}</span>
          {hideCaret ? null : <span className="select__caret">⌄</span>}
        </span>
      )}

      <button
        type="button"
        className="pick select__button"
        onClick={() => setOpen((wasOpen) => !wasOpen)}
        disabled={disabled}
        title={title}
        aria-haspopup="listbox"
        aria-expanded={open}
      >
        {current?.swatch ? renderSwatch(current.swatch) : null}
        <span className="select__label">{current?.label ?? placeholder ?? '—'}</span>
        {hideCaret ? null : (
          <span className="select__caret" aria-hidden="true">
            ⌄
          </span>
        )}
      </button>

      {open ? (
        <div className={`select__menu ${direction}`} role="listbox" ref={menuRef}>
          {options.map((option) => (
            <button
              type="button"
              key={option.value}
              role="option"
              aria-selected={option.value === value}
              className={option.value === value ? 'on' : ''}
              onClick={() => {
                setOpen(false)
                if (option.value !== value) onChange(option.value)
              }}
            >
              <span className="select__option">
                {option.swatch ? renderSwatch(option.swatch) : null}
                {option.label}
              </span>
              {option.value === value ? <span className="check">✓</span> : null}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  )
}
