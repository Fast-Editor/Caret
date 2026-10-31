import { SchemaPage } from '../SchemaPage'
import { TerminalColorsSection } from './TerminalColorsSection'
export function AppearancePage() { return <SchemaPage page="appearance" after={<TerminalColorsSection />} /> }
