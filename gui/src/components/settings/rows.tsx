import { useEffect, useState, type ReactNode } from 'react'

/** Pill switch used for boolean settings rows. */
export function Switch({
  label,
  checked,
  onChange
}: {
  label: string
  checked: boolean
  onChange: (value: boolean) => void
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className={`sw${checked ? ' on' : ''}`}
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  )
}

/**
 * One settings row: what it does on the left, its control on the right.
 * Rows stack inside a `.gcard`.
 */
export function Row({
  title,
  hint,
  children
}: {
  title: string
  hint: string
  children: ReactNode
}) {
  return (
    <div className="grow">
      <div className="grow__text">
        <b>{title}</b>
        <small>{hint}</small>
      </div>
      <div className="grow__ctl">{children}</div>
    </div>
  )
}

/** Page header shared by every settings tab that leads with a title. */
export function SettingsHeader({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <header className="set-head">
      <h2>{title}</h2>
      <p>{subtitle}</p>
    </header>
  )
}

/**
 * Number input for a settings row.
 *
 * The value is only clamped when the field is committed (blur or Enter).
 * Clamping on every keystroke looks fine but is unusable: typing `1` into a
 * 10-24 range clamps straight to 10 and rewrites the field, so the next digit
 * lands after it and the user can never type `16`.
 */
export function NumberField({
  value,
  label,
  min,
  max,
  step,
  suffix,
  disabled,
  onCommit
}: {
  value: number
  label: string
  min?: number
  max?: number
  step?: number
  suffix?: string
  disabled?: boolean
  onCommit: (value: number) => void
}) {
  const [draft, setDraft] = useState(String(value))

  // Follow outside changes (a different profile, a clamp elsewhere), but leave
  // the field alone while it is being edited.
  useEffect(() => setDraft(String(value)), [value])

  const commit = (): void => {
    onCommit(Number(draft))
    setDraft(String(value))
  }

  return (
    <span className="grow__num">
      <input
        className="inp num"
        type="number"
        inputMode="numeric"
        min={min}
        max={max}
        step={step}
        value={draft}
        disabled={disabled}
        aria-label={label}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur()
        }}
      />
      {suffix ? <small>{suffix}</small> : null}
    </span>
  )
}
