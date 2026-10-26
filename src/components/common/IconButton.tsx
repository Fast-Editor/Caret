import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react'
import { cn } from './cn'
import { Tooltip } from './Tooltip'

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: ReactNode
  label: string
  /** Show tooltip (default true). */
  tooltip?: boolean
  size?: 'xs' | 'sm' | 'md'
  active?: boolean
  shortcut?: string
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { icon, label, tooltip = true, size = 'sm', active, shortcut, className, ...rest }, ref,
) {
  const btn = (
    <button ref={ref} aria-label={label} aria-pressed={active}
      className={cn('inline-flex items-center justify-center rounded-md text-muted hover:text-fg hover:bg-hover disabled:opacity-40 disabled:pointer-events-none transition-colors shrink-0',
        size === 'xs' ? 'size-5 [&_svg]:size-3.5' : size === 'sm' ? 'size-6 [&_svg]:size-4' : 'size-7 [&_svg]:size-[18px]',
        active && 'text-fg bg-active', className)}
      {...rest}>
      {icon}
    </button>
  )
  return tooltip ? <Tooltip label={label} shortcut={shortcut}>{btn}</Tooltip> : btn
})
