import { SettingsPage } from '@/components/settings'
export function SettingsTab({ page, query }: { page?: string; query?: string }) {
  return <div className="h-full w-full overflow-auto bg-editor"><SettingsPage page={page} query={query} /></div>
}
