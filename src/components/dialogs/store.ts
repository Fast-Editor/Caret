import { create } from 'zustand'
import type { ReactNode } from 'react'

export interface ConfirmOptions { title: string; message?: ReactNode; okLabel?: string; cancelLabel?: string; danger?: boolean; detail?: string }
export interface PromptOptions { title: string; message?: ReactNode; placeholder?: string; defaultValue?: string; okLabel?: string; validate?: (v: string) => string | null | undefined; selectRange?: [number, number]; multiline?: boolean }
export interface PickItem<T = string> { id: T; label: string; description?: string; detail?: string; icon?: ReactNode; keywords?: string }
export interface PickOptions<T = string> { title: string; items: PickItem<T>[]; placeholder?: string; allowFreeText?: boolean }

export type DialogRequest =
  | { kind: 'confirm'; id: number; opts: ConfirmOptions; resolve: (v: boolean) => void }
  | { kind: 'prompt'; id: number; opts: PromptOptions; resolve: (v: string | null) => void }
  | { kind: 'pick'; id: number; opts: PickOptions<string>; resolve: (v: string | null) => void }
  | { kind: 'custom'; id: number; title: string; render: (close: () => void) => ReactNode; resolve: () => void; wide?: boolean }

interface DialogStore { queue: DialogRequest[]; push: (r: DialogRequest) => void; pop: (id: number) => void }
export const useDialogs = create<DialogStore>((set) => ({
  queue: [],
  push: (r) => set((s) => ({ queue: [...s.queue, r] })),
  pop: (id) => set((s) => ({ queue: s.queue.filter((r) => r.id !== id) })),
}))

let seq = 0
export function confirm(opts: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => useDialogs.getState().push({ kind: 'confirm', id: ++seq, opts, resolve }))
}
export function prompt(opts: PromptOptions): Promise<string | null> {
  return new Promise((resolve) => useDialogs.getState().push({ kind: 'prompt', id: ++seq, opts, resolve }))
}
export function pick<T extends string = string>(opts: PickOptions<T>): Promise<T | null> {
  return new Promise((resolve) => useDialogs.getState().push({ kind: 'pick', id: ++seq, opts: opts as unknown as PickOptions<string>, resolve: resolve as (v: string | null) => void }))
}
export function showDialog(title: string, render: (close: () => void) => ReactNode, opts?: { wide?: boolean }): Promise<void> {
  return new Promise((resolve) => useDialogs.getState().push({ kind: 'custom', id: ++seq, title, render, resolve, wide: opts?.wide }))
}
