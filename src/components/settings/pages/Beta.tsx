import { FlaskConical } from 'lucide-react'
import { SchemaPage } from '../SchemaPage'
import { fieldsForPage } from '@/lib/settingsSchema'
import { Section, Row, Toggle } from '../primitives'

export function BetaPage() {
  const fields = fieldsForPage('beta').filter((f) => f.id !== 'beta.voice' && f.id !== 'beta.designMode')
  return (
    <SchemaPage page="beta" fields={fields}
      before={<div className="mb-5 rounded-md border border-warning/40 bg-warning/10 p-3 flex gap-3 text-[11px]"><FlaskConical size={16} className="text-warning shrink-0" /><span>Beta features may change or be removed. They are safe to try and can be switched off at any time.</span></div>}
      after={
        <Section title="Coming soon">
          <Row title="Voice input" description="Dictate prompts to the agent."><Toggle checked={false} onChange={() => {}} disabled label="Voice input" /></Row>
          <Row title="Design mode" description="Visual canvas for UI iteration."><Toggle checked={false} onChange={() => {}} disabled label="Design mode" /></Row>
        </Section>
      } />
  )
}
