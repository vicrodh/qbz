import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react'

export type DropdownOption = {
  id: string
  label: string
  /** Path to a monochrome icon, tinted white by CSS. */
  icon?: string
  hint?: string
}

type DropdownProps = {
  options: DropdownOption[]
  value: string
  onChange: (id: string) => void
  ariaLabel: string
}

/**
 * A custom select: button showing the packaging icon + label, and a listbox
 * beneath it. Keyboard: arrows move, Enter/Space picks, Escape closes.
 */
export function Dropdown({ options, value, onChange, ariaLabel }: DropdownProps) {
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(() => Math.max(0, options.findIndex((o) => o.id === value)))
  const rootRef = useRef<HTMLDivElement>(null)
  const listId = useId()
  const current = options.find((o) => o.id === value) ?? options[0]

  useEffect(() => {
    if (!open) return
    const onDoc = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', onDoc)
    return () => document.removeEventListener('mousedown', onDoc)
  }, [open])

  useEffect(() => {
    if (open) setActive(Math.max(0, options.findIndex((o) => o.id === value)))
  }, [open, options, value])

  const pick = (id: string) => {
    onChange(id)
    setOpen(false)
  }

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (!open && (event.key === 'ArrowDown' || event.key === 'ArrowUp' || event.key === 'Enter' || event.key === ' ')) {
      event.preventDefault()
      setOpen(true)
      return
    }
    if (!open) return
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActive((i) => (i + 1) % options.length)
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActive((i) => (i - 1 + options.length) % options.length)
    } else if (event.key === 'Home') {
      event.preventDefault()
      setActive(0)
    } else if (event.key === 'End') {
      event.preventDefault()
      setActive(options.length - 1)
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      pick(options[active].id)
    } else if (event.key === 'Escape') {
      event.preventDefault()
      setOpen(false)
    }
  }

  return (
    <div className={`dropdown ${open ? 'dropdown--open' : ''}`} ref={rootRef}>
      <button
        type="button"
        className="dropdown__button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        aria-label={ariaLabel}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={onKeyDown}
      >
        {current.icon && <img className="dropdown__icon" src={current.icon} alt="" width={20} height={20} />}
        <span className="dropdown__label">{current.label}</span>
        <svg className="dropdown__chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M6 9l6 6 6-6" />
        </svg>
      </button>
      {open && (
        <ul id={listId} className="dropdown__list" role="listbox" aria-label={ariaLabel} tabIndex={-1} onKeyDown={onKeyDown}>
          {options.map((option, index) => (
            <li
              key={option.id}
              role="option"
              aria-selected={option.id === value}
              className={`dropdown__option ${index === active ? 'dropdown__option--active' : ''}`}
              onMouseEnter={() => setActive(index)}
              onClick={() => pick(option.id)}
            >
              {option.icon ? <img className="dropdown__icon" src={option.icon} alt="" width={20} height={20} /> : <span className="dropdown__icon" />}
              <span>
                {option.label}
                {option.hint && <small className="dropdown__hint">{option.hint}</small>}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
