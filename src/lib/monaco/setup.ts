/**
 * Monaco bootstrap: workers, themes, TS defaults, settings-driven editor options.
 * Import `monaco` from here (re-exported) so initialization is guaranteed.
 */
import * as monaco from 'monaco-editor'
import editorWorker from 'monaco-editor/editor/editor.worker.js?worker'
import jsonWorker from 'monaco-editor/language/json/json.worker.js?worker'
import cssWorker from 'monaco-editor/language/css/css.worker.js?worker'
import htmlWorker from 'monaco-editor/language/html/html.worker.js?worker'
import tsWorker from 'monaco-editor/language/typescript/ts.worker.js?worker'
import { THEMES } from './themes'
import { useSettings } from '@/store/settings'
import type { UiSettings, ThemeId } from '@/types'

export { monaco }

let initialized = false
let currentTheme: string = 'caret-dark'

export function initMonaco(): typeof monaco {
  if (initialized) return monaco
  initialized = true
  ;(self as unknown as { MonacoEnvironment: unknown }).MonacoEnvironment = {
    getWorker(_: string, label: string) {
      switch (label) {
        case 'json': return new jsonWorker()
        case 'css': case 'scss': case 'less': return new cssWorker()
        case 'html': case 'handlebars': case 'razor': return new htmlWorker()
        case 'typescript': case 'javascript': return new tsWorker()
        default: return new editorWorker()
      }
    },
  }
  for (const [id, data] of Object.entries(THEMES)) monaco.editor.defineTheme(id, data)

  const ts = monaco.typescript
  const compiler: Parameters<typeof ts.typescriptDefaults.setCompilerOptions>[0] = {
    target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.NodeJs,
    jsx: ts.JsxEmit.ReactJSX, allowJs: true, allowNonTsExtensions: true, noEmit: true, esModuleInterop: true, skipLibCheck: true,
    strict: false, allowSyntheticDefaultImports: true, resolveJsonModule: true, isolatedModules: true, lib: ['esnext', 'dom', 'dom.iterable'],
  }
  ts.typescriptDefaults.setCompilerOptions(compiler)
  ts.javascriptDefaults.setCompilerOptions({ ...compiler, checkJs: false })
  // Missing modules are expected (no node_modules resolution in the browser) — hide 2307/2792 noise, keep real errors.
  const ignore = [2307, 2792, 7016, 2691, 1479, 2580, 2584, 2503, 6133, 6192]
  ts.typescriptDefaults.setDiagnosticsOptions({ noSemanticValidation: false, noSyntaxValidation: false, diagnosticCodesToIgnore: ignore })
  ts.javascriptDefaults.setDiagnosticsOptions({ noSemanticValidation: true, noSyntaxValidation: false })
  ts.typescriptDefaults.setEagerModelSync(true)
  ts.javascriptDefaults.setEagerModelSync(true)
  monaco.json.jsonDefaults.setDiagnosticsOptions({ validate: true, allowComments: true, trailingCommas: 'ignore', schemaValidation: 'warning' })

  // Register a few languages Monaco lacks so files still get a sensible id (plaintext tokens).
  for (const id of ['toml', 'zig', 'nim', 'prisma', 'log', 'wgsl', 'hlsl', 'makefile', 'cmake', 'hcl', 'ocaml', 'haskell', 'erlang', 'elm', 'latex', 'bibtex', 'diff', 'vb']) {
    if (!monaco.languages.getLanguages().some((l) => l.id === id)) {
      monaco.languages.register({ id })
      monaco.languages.setMonarchTokensProvider(id, genericMonarch(id))
      monaco.languages.setLanguageConfiguration(id, { comments: { lineComment: id === 'latex' || id === 'bibtex' ? '%' : (id === 'haskell' || id === 'elm' ? '--' : (id === 'ocaml' ? undefined : '#')), blockComment: id === 'ocaml' ? ['(*', '*)'] : ['/*', '*/'] }, brackets: [['{', '}'], ['[', ']'], ['(', ')']], autoClosingPairs: [{ open: '{', close: '}' }, { open: '[', close: ']' }, { open: '(', close: ')' }, { open: '"', close: '"' }, { open: "'", close: "'" }] })
    }
  }
  applyTheme(useSettings.getState().settings?.ui.theme ?? 'caret-dark')
  useSettings.subscribe((s, prev) => {
    if (s.settings?.ui.theme && s.settings.ui.theme !== prev.settings?.ui.theme) applyTheme(s.settings.ui.theme)
  })
  return monaco
}

function genericMonarch(id: string): monaco.languages.IMonarchLanguage {
  const kw = id === 'toml' ? ['true', 'false'] : id === 'zig' ? ['const', 'var', 'fn', 'pub', 'return', 'if', 'else', 'while', 'for', 'switch', 'struct', 'enum', 'union', 'try', 'catch', 'defer', 'errdefer', 'comptime', 'inline', 'test', 'and', 'or', 'break', 'continue', 'null', 'undefined', 'true', 'false', 'unreachable', 'async', 'await', 'export', 'extern', 'import', 'usingnamespace'] : id === 'hcl' ? ['resource', 'variable', 'output', 'module', 'provider', 'terraform', 'locals', 'data', 'true', 'false', 'null', 'for', 'in', 'if'] : id === 'haskell' || id === 'elm' ? ['module', 'where', 'import', 'data', 'type', 'class', 'instance', 'let', 'in', 'case', 'of', 'if', 'then', 'else', 'do', 'newtype', 'deriving', 'exposing', 'port', 'as'] : id === 'ocaml' ? ['let', 'in', 'rec', 'match', 'with', 'fun', 'function', 'type', 'module', 'struct', 'sig', 'end', 'if', 'then', 'else', 'open', 'begin', 'mutable', 'of', 'and', 'true', 'false'] : id === 'prisma' ? ['model', 'enum', 'datasource', 'generator', 'type'] : ['true', 'false', 'null', 'if', 'else', 'end', 'return', 'function', 'let', 'const', 'var', 'for', 'while', 'in', 'do', 'then', 'import', 'export', 'struct', 'fn', 'pub', 'use', 'mod']
  const lineComment = id === 'latex' || id === 'bibtex' ? '%' : id === 'haskell' || id === 'elm' ? '--' : id === 'zig' || id === 'wgsl' || id === 'hlsl' || id === 'prisma' ? '//' : '#'
  return {
    keywords: kw, tokenizer: {
      root: [
        [new RegExp(`${lineComment.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}.*$`), 'comment'],
        [/\/\*/, 'comment', '@comment'], [/"([^"\\]|\\.)*$/, 'string.invalid'], [/"/, 'string', '@string'], [/'[^']*'/, 'string'],
        [/\b\d+(\.\d+)?([eE][-+]?\d+)?\b/, 'number'], [/0x[0-9a-fA-F]+/, 'number.hex'],
        [/^\[[^\]]*\]/, 'type'], [/^[A-Za-z_][\w.-]*(?=\s*=)/, 'attribute.name'],
        [/[A-Za-z_][\w]*/, { cases: { '@keywords': 'keyword', '@default': 'identifier' } }],
        [/[{}()[\]]/, '@brackets'], [/[=<>!+\-*/%&|^~?:]+/, 'operator'],
        ...(id === 'diff' ? [[/^\+.*$/, 'meta.diff.add'], [/^-.*$/, 'meta.diff.delete'], [/^@@.*$/, 'meta.diff.range'], [/^(diff|index|---|\+\+\+).*$/, 'meta.diff.header']] as monaco.languages.IMonarchLanguageRule[] : []),
        ...(id === 'log' ? [[/\b(ERROR|FATAL|SEVERE)\b.*$/, 'invalid'], [/\b(WARN|WARNING)\b/, 'keyword'], [/\b(INFO|DEBUG|TRACE)\b/, 'comment'], [/\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}[.\d]*Z?/, 'number']] as monaco.languages.IMonarchLanguageRule[] : []),
      ],
      comment: [[/[^/*]+/, 'comment'], [/\*\//, 'comment', '@pop'], [/[/*]/, 'comment']],
      string: [[/[^\\"]+/, 'string'], [/\\./, 'string.escape'], [/"/, 'string', '@pop']],
    },
  }
}

export function applyTheme(theme: ThemeId | string): void {
  currentTheme = THEMES[theme] ? theme : 'caret-dark'
  monaco.editor.setTheme(currentTheme)
  const root = document.documentElement
  for (const cls of Array.from(root.classList)) if (cls.startsWith('theme-')) root.classList.remove(cls)
  root.classList.add(`theme-${currentTheme}`)
  root.classList.toggle('dark', currentTheme !== 'caret-light')
}
export function getTheme(): string { return currentTheme }

/** Editor options derived from settings.ui (applied to every editor; reactive via useEditorOptions). */
export function editorOptionsFromSettings(ui: UiSettings | undefined): monaco.editor.IEditorOptions & monaco.editor.IGlobalEditorOptions {
  const u = ui ?? ({} as UiSettings)
  return {
    fontFamily: u.fontFamily || 'JetBrains Mono, Menlo, monospace', fontSize: u.fontSize || 13, lineHeight: Math.round((u.fontSize || 13) * (u.lineHeight || 1.6)),
    fontLigatures: u.fontLigatures ?? true, tabSize: u.tabSize || 2, insertSpaces: u.insertSpaces ?? true, detectIndentation: true,
    wordWrap: u.wordWrap ?? 'off', minimap: { enabled: u.minimap ?? true, renderCharacters: false, maxColumn: 100, scale: 1 },
    renderWhitespace: u.renderWhitespace ?? 'selection', cursorBlinking: u.cursorBlinking ?? 'smooth', cursorStyle: u.cursorStyle ?? 'line', cursorSmoothCaretAnimation: u.reduceMotion ? 'off' : 'on',
    bracketPairColorization: { enabled: u.bracketPairColorization ?? true }, guides: { bracketPairs: 'active', indentation: true },
    stickyScroll: { enabled: u.stickyScroll ?? true }, lineNumbers: u.lineNumbers ?? 'on', smoothScrolling: u.smoothScrolling ?? true,
    scrollBeyondLastLine: true, automaticLayout: true, padding: { top: 6 }, renderLineHighlight: 'line', occurrencesHighlight: 'singleFile', selectionHighlight: true,
    suggest: { showStatusBar: true, preview: true, previewMode: 'subwordSmart', showInlineDetails: true, insertMode: 'insert' }, quickSuggestions: { other: true, comments: false, strings: false }, suggestOnTriggerCharacters: true, acceptSuggestionOnEnter: 'on', tabCompletion: 'off',
    inlineSuggest: ({ enabled: true, showToolbar: 'onHover', mode: 'subword', suppressSuggestions: false, edits: { allowCodeShifting: 'always', renderSideBySide: 'auto', showCollapsed: false } }) as unknown as monaco.editor.IInlineSuggestOptions,
    inlayHints: { enabled: 'on', fontSize: Math.max(10, (u.fontSize || 13) - 2) }, linkedEditing: true, formatOnPaste: false, formatOnType: false,
    scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10, useShadows: false }, overviewRulerBorder: false, hideCursorInOverviewRuler: false, renderFinalNewline: 'dimmed',
    unicodeHighlight: { ambiguousCharacters: false }, dropIntoEditor: { enabled: true }, multiCursorModifier: 'alt', codeLens: true, folding: true, foldingHighlight: false, showFoldingControls: 'mouseover', matchBrackets: 'always', accessibilitySupport: 'off', contextmenu: false,
  }
}
