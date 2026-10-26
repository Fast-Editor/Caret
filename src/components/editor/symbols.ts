import { monaco } from '@/lib/monaco/setup'
import { lspManager } from '@/lib/lsp/manager'
import { symbolsToMonaco } from '@/lib/lsp/providers'
import type { DocumentSymbol as LspDocumentSymbol } from '@/lib/lsp/types'
import { pathToUri } from '@/lib/lsp/convert'

export interface FlatSymbol { id: string; name: string; detail: string; kind: monaco.languages.SymbolKind; range: monaco.Range; selectionRange: monaco.Range; depth: number; children: FlatSymbol[] }

/** Document symbols: LSP server when running, else Monaco's built-in providers (TS/JSON/CSS/HTML workers). */
export async function getDocumentSymbols(model: monaco.editor.ITextModel): Promise<monaco.languages.DocumentSymbol[]> {
  const client = lspManager.clientForModel(model)
  if (client?.capabilities.documentSymbolProvider) {
    try { const r = await client.request<LspDocumentSymbol[] | null>('textDocument/documentSymbol', { textDocument: { uri: pathToUri(model.uri.path) } }, 10000); if (r) return symbolsToMonaco(r) } catch { /* fall through */ }
  }
  // Monaco built-in: use the (internal but stable) quick outline command fallback via TS worker for ts/js
  const lang = model.getLanguageId()
  if (lang === 'typescript' || lang === 'javascript') {
    try {
      const getWorker = lang === 'typescript' ? monaco.typescript.getTypeScriptWorker : monaco.typescript.getJavaScriptWorker
      const worker = await (await getWorker())(model.uri)
      const items = (await worker.getNavigationTree(model.uri.toString())) as TsNavTree | undefined
      if (items) return (items.childItems ?? []).map((c) => navToSymbol(c, model))
    } catch { /* ignore */ }
  }
  return regexOutline(model)
}

interface TsNavTree { text: string; kind: string; spans: { start: number; length: number }[]; nameSpan?: { start: number; length: number }; childItems?: TsNavTree[] }
const TS_KIND: Record<string, monaco.languages.SymbolKind> = { module: 1, class: 4, interface: 10, enum: 9, function: 11, method: 5, property: 6, getter: 6, setter: 6, var: 12, let: 12, const: 13, constructor: 8, 'type': 25 as monaco.languages.SymbolKind, alias: 12, parameter: 12, 'enum member': 21, 'local function': 11, 'local var': 12 }
function navToSymbol(n: TsNavTree, model: monaco.editor.ITextModel): monaco.languages.DocumentSymbol {
  const span = n.spans[0] ?? { start: 0, length: 0 }
  const s = model.getPositionAt(span.start), e = model.getPositionAt(span.start + span.length)
  const ns = n.nameSpan ? model.getPositionAt(n.nameSpan.start) : s, ne = n.nameSpan ? model.getPositionAt(n.nameSpan.start + n.nameSpan.length) : e
  return { name: n.text, detail: '', kind: TS_KIND[n.kind] ?? (12 as monaco.languages.SymbolKind), tags: [], range: new monaco.Range(s.lineNumber, s.column, e.lineNumber, e.column), selectionRange: new monaco.Range(ns.lineNumber, ns.column, ne.lineNumber, ne.column), children: (n.childItems ?? []).filter((c) => c.kind !== 'alias').map((c) => navToSymbol(c, model)) }
}

/** Very light regex outline for languages without a server (functions/classes/headings). */
function regexOutline(model: monaco.editor.ITextModel): monaco.languages.DocumentSymbol[] {
  const lang = model.getLanguageId(); const out: monaco.languages.DocumentSymbol[] = []
  const lines = model.getLinesContent(); if (lines.length > 20000) return []
  const push = (name: string, kind: monaco.languages.SymbolKind, i: number) => { out.push({ name, detail: '', kind, tags: [], range: new monaco.Range(i + 1, 1, i + 1, lines[i].length + 1), selectionRange: new monaco.Range(i + 1, 1, i + 1, lines[i].length + 1) }) }
  const K = { fn: 11 as monaco.languages.SymbolKind, cls: 4 as monaco.languages.SymbolKind, str: 14 as monaco.languages.SymbolKind, mod: 1 as monaco.languages.SymbolKind }
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i]
    let m: RegExpExecArray | null
    if (lang === 'markdown') { if ((m = /^(#{1,6})\s+(.*)$/.exec(l))) push(m[2].trim(), K.str, i); continue }
    if (lang === 'python') { if ((m = /^\s*(?:async\s+)?def\s+(\w+)/.exec(l))) push(m[1], K.fn, i); else if ((m = /^\s*class\s+(\w+)/.exec(l))) push(m[1], K.cls, i); continue }
    if (lang === 'rust') { if ((m = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(\w+)/.exec(l))) push(m[1], K.fn, i); else if ((m = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|type|mod)\s+(\w+)/.exec(l))) push(m[1], K.cls, i); else if ((m = /^\s*impl(?:<[^>]*>)?\s+(?:[\w:<>]+\s+for\s+)?([\w:<>]+)/.exec(l))) push(`impl ${m[1]}`, K.mod, i); continue }
    if (lang === 'go') { if ((m = /^func\s+(?:\([^)]*\)\s*)?(\w+)/.exec(l))) push(m[1], K.fn, i); else if ((m = /^type\s+(\w+)/.exec(l))) push(m[1], K.cls, i); continue }
    if (lang === 'java' || lang === 'csharp' || lang === 'kotlin' || lang === 'swift' || lang === 'cpp' || lang === 'c') { if ((m = /^\s*(?:public|private|protected|internal|static|final|override|open|export|fun|func)?\s*(?:class|struct|interface|enum|protocol|object)\s+(\w+)/.exec(l))) push(m[1], K.cls, i); else if ((m = /^\s*(?:[\w<>[\],\s]+\s+)?(\w+)\s*\([^;]*\)\s*(?:const\s*)?(?:throws\s+\w+\s*)?\{?\s*$/.exec(l)) && !/^\s*(if|for|while|switch|return|else|catch)\b/.test(l)) push(m[1], K.fn, i); continue }
    if (lang === 'ruby') { if ((m = /^\s*def\s+([\w.?!]+)/.exec(l))) push(m[1], K.fn, i); else if ((m = /^\s*(?:class|module)\s+([\w:]+)/.exec(l))) push(m[1], K.cls, i); continue }
    if (lang === 'shell') { if ((m = /^\s*(?:function\s+)?(\w+)\s*\(\)\s*\{?/.exec(l))) push(m[1], K.fn, i); continue }
    if (lang === 'css' || lang === 'scss' || lang === 'less') { if ((m = /^([.#]?[\w-]+(?:[\s>+~,.:#\w[\]="'-]*)?)\s*\{\s*$/.exec(l))) push(m[1].trim(), K.cls, i); continue }
    if (lang === 'yaml' || lang === 'json' || lang === 'ini') { if ((m = /^(\w[\w.-]*)\s*[:=]/.exec(l)) || (m = /^\[([^\]]+)\]/.exec(l))) push(m[1], K.str, i); continue }
  }
  return out
}

export function flattenSymbols(syms: monaco.languages.DocumentSymbol[], depth = 0, prefix = ''): FlatSymbol[] {
  const out: FlatSymbol[] = []
  syms.forEach((s, i) => {
    const id = `${prefix}${i}`
    const range = monaco.Range.lift(s.range); const selectionRange = monaco.Range.lift(s.selectionRange)
    const children = flattenSymbols(s.children ?? [], depth + 1, id + '.')
    out.push({ id, name: s.name, detail: s.detail, kind: s.kind, range, selectionRange, depth, children: children.filter((c) => c.depth === depth + 1) })
    out.push(...children)
  })
  return out
}
