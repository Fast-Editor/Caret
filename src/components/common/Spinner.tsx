import { cn } from './cn'
export function Spinner({ className, size = 14 }: { className?: string; size?: number }) {
  return <span role="status" aria-label="Loading" style={{ width: size, height: size }} className={cn('inline-block rounded-full border-2 border-muted border-t-transparent spin shrink-0', className)} />
}
