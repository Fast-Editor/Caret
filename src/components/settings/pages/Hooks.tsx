import { useEffect } from 'react'
import { Bot, BookOpen, ExternalLink, FileText, ListChecks, Plus, Webhook } from 'lucide-react'
import { initRulesStore, useRules } from '@/store/rules'
import { useWorkspace } from '@/store/workspace'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import type { PlanInfo } from '@/types'
import { Section, Button, IconButton, Badge, Empty, promptDialog, notify, errorMessage, relativeTime } from '../primitives'

const HOOKS_TEMPLATE = `{
  "version": 1,
  "hooks": {
    "beforeShellExecution": [{ "command": "./.caret/hooks/check-command.sh", "timeout": 10 }],
    "afterFileEdit": [{ "command": "./.caret/hooks/format.sh" }],
    "beforeSubmitPrompt": [],
    "beforeMCPExecution": [],
    "beforeReadFile": [],
    "afterAgentResponse": [],
    "stop": []
  }
}
`
const COMMAND_TEMPLATE = (name: string) => `---\ndescription: ${name.replace(/-/g, ' ')}\nargument-hint: <what to pass>\n---\n\nDo the following with $ARGUMENTS:\n\n1. \n`
const SUBAGENT_TEMPLATE = (name: string) => `---\nname: ${name}\ndescription: When to use this subagent (the main agent reads this).\ntools: read_file, grep_search, codebase_search\n---\n\nYou are a focused subagent. Return a concise report.\n`

const EVENT_HELP: Record<string, string> = {
  beforeSubmitPrompt: 'Inspect or rewrite the prompt before it is sent', beforeShellExecution: 'Allow/deny/ask for terminal commands', beforeMCPExecution: 'Gate MCP tool calls', beforeReadFile: 'Block reads of sensitive files',
  afterFileEdit: 'Run formatters/linters after the agent edits a file', afterAgentResponse: 'Post-process or log responses', afterAgentThought: 'Observe reasoning summaries', stop: 'Decide whether the agent should continue', sessionStart: 'Session lifecycle', sessionEnd: 'Session lifecycle',
}

export function HooksPage() {
  const { hooks, commands, skills, subagents, plans } = useRules()
  const reload = useRules((s) => s.loadAutomation)
  const ws = useWorkspace((s) => s.workspace)
  useEffect(() => { initRulesStore() }, [])

  const createFile = async (rel: string, content: string, open = true) => {
    if (!ws) return
    const p = `${ws.root}/${rel}`
    try { if (!(await api.fs.exists(p))) await api.fs.writeFile(p, content); await reload(); if (open) await runCommand('editor.openFile', { path: p }) } catch (e) { notify('error', `Could not create ${rel}`, errorMessage(e)) }
  }
  const newNamed = async (what: string, dir: string, tpl: (n: string) => string) => {
    const name = await promptDialog({ title: `New ${what}`, message: 'Lowercase name with dashes.', placeholder: 'my-' + what, validate: (v) => /^[a-z0-9][a-z0-9-]*$/.test(v) ? null : 'Use a-z, 0-9 and dashes' })
    if (name) await createFile(`${dir}/${name}.md`, tpl(name))
  }
  const byEvent = Object.entries(hooks.reduce<Record<string, typeof hooks>>((acc, h) => { (acc[h.event] ??= []).push(h); return acc }, {}))

  return (
    <>
      <Section title="Hooks" description="Shell commands that run at agent lifecycle points. They receive JSON on stdin and may return { permission: allow | deny | ask, userMessage, agentMessage }." actions={<><Button disabled={!ws} onClick={() => createFile('.caret/hooks.json', HOOKS_TEMPLATE)}><ExternalLink size={12} />{hooks.length ? 'Open hooks.json' : 'Create hooks.json'}</Button></>}>
        {hooks.length === 0 && <Empty icon={<Webhook size={22} />} title="No hooks configured" description={<>Define hooks in <code className="mono">.caret/hooks.json</code>, <code className="mono">.cursor/hooks.json</code> or <code className="mono">~/.caret/hooks.json</code>.</>} />}
        {byEvent.map(([event, list]) => (
          <div key={event} className="px-3 py-2">
            <div className="text-xs font-medium flex items-center gap-2">{event}<span className="text-[10px] text-muted font-normal">{EVENT_HELP[event]}</span></div>
            {list.map((h, i) => <div key={i} className="text-[11px] mono text-muted truncate-1 pl-2">{h.command}<span className="text-faint"> · {h.source}{h.timeoutMs ? ` · ${h.timeoutMs} ms` : ''}{h.matcher ? ` · matcher: ${h.matcher}` : ''}</span></div>)}
          </div>
        ))}
      </Section>

      <Section title="Slash commands" description="Reusable prompts invoked with /name in chat. Markdown files with optional frontmatter; $ARGUMENTS is substituted." actions={<Button disabled={!ws} onClick={() => newNamed('command', '.caret/commands', COMMAND_TEMPLATE)}><Plus size={12} />New command</Button>}>
        {commands.length === 0 && <Empty icon={<ListChecks size={22} />} title="No commands" description="Create .caret/commands/<name>.md (Cursor's .cursor/commands works too)." />}
        {commands.map((c) => <ListRow key={`${c.source}-${c.name}`} title={`/${c.name}`} subtitle={c.description} badge={c.source} hint={c.argumentHint} path={c.path} />)}
      </Section>

      <Section title="Skills" description="SKILL.md folders the agent can load on demand (via its skill tool) from .caret/skills, .cursor/skills, ~/.caret/skills and ~/.claude/skills.">
        {skills.length === 0 && <Empty icon={<BookOpen size={22} />} title="No skills found" />}
        {skills.map((s) => <ListRow key={s.id} title={s.name} subtitle={s.description} badge={s.source} path={s.path} />)}
      </Section>

      <Section title="Subagents" description="Specialised agents the main agent can delegate to with the task tool." actions={<Button disabled={!ws} onClick={() => newNamed('subagent', '.caret/agents', SUBAGENT_TEMPLATE)}><Plus size={12} />New subagent</Button>}>
        {subagents.length === 0 && <Empty icon={<Bot size={22} />} title="No subagents" description="Create .caret/agents/<name>.md with name/description/tools frontmatter." />}
        {subagents.map((s) => <ListRow key={s.id} title={s.name} subtitle={s.description} badge={s.source} hint={s.modelId} path={s.path} />)}
      </Section>

      <Section title="Plans" description="Plans written in Plan mode live in .caret/plans. Open one to review, edit and build it.">
        {plans.length === 0 && <Empty icon={<FileText size={22} />} title="No plans yet" description="Switch the chat to Plan mode and ask for an implementation plan." />}
        {plans.map((p) => <PlanRow key={p.path} p={p} />)}
      </Section>
    </>
  )
}

function ListRow({ title, subtitle, badge, hint, path }: { title: string; subtitle?: string; badge?: string; hint?: string; path?: string }) {
  return (
    <div className="px-3 py-2 flex items-center gap-3">
      <div className="flex-1 min-w-0"><div className="text-xs font-medium flex items-center gap-2 mono">{title}{badge && <Badge>{badge}</Badge>}{hint && <span className="text-[10px] text-faint font-normal">{hint}</span>}</div>{subtitle && <div className="text-[11px] text-muted truncate-1">{subtitle}</div>}</div>
      {path && <IconButton title="Open" onClick={() => void runCommand('editor.openFile', { path })}><ExternalLink size={12} /></IconButton>}
    </div>
  )
}

function PlanRow({ p }: { p: PlanInfo }) {
  const tone = p.status === 'done' ? 'success' : p.status === 'building' ? 'accent' : p.status === 'approved' ? 'info' : 'default'
  return (
    <div className="px-3 py-2 flex items-center gap-3">
      <div className="flex-1 min-w-0"><div className="text-xs font-medium flex items-center gap-2">{p.title}<Badge tone={tone}>{p.status}</Badge></div><div className="text-[11px] text-muted truncate-1">{p.path} · {relativeTime(p.updatedAt)}</div></div>
      <Button size="sm" onClick={() => void runCommand('editor.openPlan', { path: p.path })}>Open</Button>
    </div>
  )
}
