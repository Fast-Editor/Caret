import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react'
import { cn } from './cn'
import './motion.css'

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'link'
export type ButtonSize = 'xs' | 'sm' | 'md'

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant
  size?: ButtonSize
  icon?: ReactNode
  loading?: boolean
}

const variants: Record<ButtonVariant, string> = {
  primary: 'bg-accent text-accent-fg hover:bg-accent-hover border-transparent press',
  secondary: 'bg-input text-fg hover:bg-active border-border',
  ghost: 'bg-transparent text-fg hover:bg-hover border-transparent',
  danger: 'bg-danger/90 text-white hover:bg-danger border-transparent',
  link: 'bg-transparent text-accent hover:underline border-transparent px-0',
}
const sizes: Record<ButtonSize, string> = { xs: 'h-5 px-1.5 text-[11px] gap-1', sm: 'h-6 px-2 text-xs gap-1.5', md: 'h-7 px-3 text-[13px] gap-2' }

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = 'secondary', size = 'sm', icon, loading, className, children, disabled, ...rest }, ref,
) {
  return (
    <button ref={ref} disabled={disabled || loading}
      className={cn('inline-flex items-center justify-center rounded-md border select-none whitespace-nowrap transition-colors disabled:opacity-50 disabled:pointer-events-none', variants[variant], sizes[size], className)}
      {...rest}>
      {loading ? <span className="size-3 rounded-full border-2 border-current border-t-transparent spin" /> : icon}
      {children}
    </button>
  )
})
