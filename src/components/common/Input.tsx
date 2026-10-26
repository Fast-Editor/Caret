import { forwardRef, type InputHTMLAttributes, type ReactNode } from 'react'
import { cn } from './cn'

export interface InputProps extends InputHTMLAttributes<HTMLInputElement> { leading?: ReactNode; trailing?: ReactNode; invalid?: boolean; wrapperClassName?: string }

export const Input = forwardRef<HTMLInputElement, InputProps>(function Input({ leading, trailing, invalid, className, wrapperClassName, ...rest }, ref) {
  return (
    <div className={cn('flex items-center gap-1.5 h-7 rounded-md border bg-input px-2 text-xs focus-within:border-accent', invalid ? 'border-danger' : 'border-border', wrapperClassName)}>
      {leading && <span className="text-muted shrink-0 [&_svg]:size-3.5">{leading}</span>}
      <input ref={ref} className={cn('flex-1 min-w-0 bg-transparent outline-none placeholder:text-faint', className)} spellCheck={false} autoCorrect="off" autoCapitalize="off" {...rest} />
      {trailing && <span className="shrink-0 flex items-center gap-0.5">{trailing}</span>}
    </div>
  )
})
