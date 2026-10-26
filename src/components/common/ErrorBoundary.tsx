import React from 'react'

interface Props { name?: string; children: React.ReactNode; fallback?: (error: Error, reset: () => void) => React.ReactNode }
interface State { error: Error | null }

/** Catches render errors so one broken region never blanks the whole workbench. */
export class ErrorBoundary extends React.Component<Props, State> {
  state: State = { error: null }
  static getDerivedStateFromError(error: Error): State { return { error } }
  componentDidCatch(error: Error, info: React.ErrorInfo): void { console.error(`[${this.props.name ?? 'ui'}] render error`, error, info.componentStack) }
  reset = (): void => this.setState({ error: null })
  render(): React.ReactNode {
    const { error } = this.state
    if (!error) return this.props.children
    if (this.props.fallback) return this.props.fallback(error, this.reset)
    return (
      <div className="h-full w-full flex items-center justify-center p-6">
        <div className="max-w-md rounded-md border border-border bg-panel p-4 text-xs">
          <div className="font-medium text-danger mb-1">{this.props.name ?? 'This view'} crashed</div>
          <pre className="mono whitespace-pre-wrap text-muted max-h-40 overflow-auto selectable">{error.message}</pre>
          <button className="mt-3 px-2 py-1 rounded bg-hover hover:bg-active text-fg" onClick={this.reset}>Try again</button>
        </div>
      </div>
    )
  }
}
