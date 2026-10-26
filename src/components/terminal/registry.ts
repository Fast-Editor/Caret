import type { IMarker, Terminal } from '@xterm/xterm'
import type { FitAddon } from '@xterm/addon-fit'
import type { SearchAddon } from '@xterm/addon-search'

/** xterm markers for one semantic block, keyed by the backend block `index`. */
export interface BlockMarkers { start: IMarker; output?: IMarker; end?: IMarker }
export interface BlockHandle {
  markers: Map<number, BlockMarkers>
  /** Inclusive buffer-line range of a block (end = last buffer line while running); null when scrolled out. */
  rangeOf: (index: number) => [number, number] | null
  selectBlock: (index: number) => void
  blockAtRow: (bufferRow: number) => number | null
  /** ⌘↑ / ⌘↓ */
  navigate: (dir: 1 | -1) => void
  /** Buffer row under a client Y coordinate. */
  rowAt: (clientY: number) => number | null
  subscribe: (fn: () => void) => () => void
}
export interface TerminalHandle {
  term: Terminal; fit: FitAddon; search: SearchAddon; sessionId: string; ptyId: () => string | null
  focus: () => void; write: (data: string) => void; clear: () => void; sendInput: (data: string) => void
  blocks: BlockHandle
}
/** Non-reactive registry of live xterm instances keyed by session id. */
export const terminalRegistry = new Map<string, TerminalHandle>()
