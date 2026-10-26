import { Tooltip as RT } from 'radix-ui'
import type { ReactNode } from 'react'
import { Kbd } from './Kbd'

export function TooltipProvider({ children }: { children: ReactNode }) {
  return <RT.Provider delayDuration={500} skipDelayDuration={200}>{children}</RT.Provider>
}

export function Tooltip({ label, shortcut, side = 'bottom', children }: { label: ReactNode; shortcut?: string; side?: 'top' | 'bottom' | 'left' | 'right'; children: ReactNode }) {
  if (!label) return <>{children}</>
  return (
    <RT.Root>
      <RT.Trigger asChild>{children}</RT.Trigger>
      <RT.Portal>
        <RT.Content side={side} sideOffset={6} collisionPadding={8}
          className="z-[1000] rounded-md border border-border bg-tooltip px-2 py-1 text-xs text-fg shadow-[var(--shadow)] flex items-center gap-2 max-w-xs">
          <span>{label}</span>
          {shortcut && <Kbd keys={shortcut} />}
        </RT.Content>
      </RT.Portal>
    </RT.Root>
  )
}
