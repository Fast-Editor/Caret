/**
 * File → Monaco language id mapping, plus Monaco → LSP language id mapping.
 * Pure module (no Monaco import) so it is testable and cheap.
 */

const EXT: Record<string, string> = {
  ts: 'typescript', mts: 'typescript', cts: 'typescript', tsx: 'typescript',
  js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'javascript', es6: 'javascript',
  json: 'json', jsonc: 'json', json5: 'json', webmanifest: 'json', babelrc: 'json', eslintrc: 'json', prettierrc: 'json', har: 'json', ipynb: 'json', geojson: 'json', avsc: 'json',
  html: 'html', htm: 'html', xhtml: 'html', vue: 'html', svelte: 'html', astro: 'html', ejs: 'html', hbs: 'handlebars', handlebars: 'handlebars', mustache: 'handlebars', njk: 'html', jinja: 'html', jinja2: 'html', twig: 'twig', erb: 'html', liquid: 'liquid',
  css: 'css', scss: 'scss', sass: 'scss', less: 'less', pcss: 'css', postcss: 'css', styl: 'css',
  md: 'markdown', markdown: 'markdown', mdx: 'markdown', mdown: 'markdown', mkd: 'markdown', rmd: 'markdown',
  py: 'python', pyi: 'python', pyw: 'python', gyp: 'python', pyx: 'python', ipy: 'python',
  rs: 'rust', go: 'go', java: 'java', kt: 'kotlin', kts: 'kotlin', scala: 'scala', sc: 'scala', groovy: 'java', gradle: 'java',
  c: 'c', h: 'c', cc: 'cpp', cpp: 'cpp', cxx: 'cpp', hpp: 'cpp', hh: 'cpp', hxx: 'cpp', ino: 'cpp', cu: 'cpp', cuh: 'cpp', m: 'objective-c', mm: 'objective-c',
  cs: 'csharp', csx: 'csharp', fs: 'fsharp', fsx: 'fsharp', fsi: 'fsharp', vb: 'vb',
  rb: 'ruby', rbw: 'ruby', rake: 'ruby', gemspec: 'ruby', ru: 'ruby', erbs: 'ruby',
  php: 'php', phtml: 'php', php5: 'php', inc: 'php',
  swift: 'swift', dart: 'dart', lua: 'lua', r: 'r', rmarkdown: 'r', jl: 'julia', pl: 'perl', pm: 'perl', t: 'perl', raku: 'perl',
  sh: 'shell', bash: 'shell', zsh: 'shell', fish: 'shell', ksh: 'shell', command: 'shell', bats: 'shell',
  ps1: 'powershell', psm1: 'powershell', psd1: 'powershell', bat: 'bat', cmd: 'bat',
  yml: 'yaml', yaml: 'yaml', toml: 'ini', ini: 'ini', cfg: 'ini', conf: 'ini', properties: 'ini', editorconfig: 'ini', env: 'ini', npmrc: 'ini', gitconfig: 'ini', desktop: 'ini',
  xml: 'xml', xsd: 'xml', xsl: 'xml', xslt: 'xml', svg: 'xml', plist: 'xml', csproj: 'xml', fsproj: 'xml', vbproj: 'xml', props: 'xml', targets: 'xml', nuspec: 'xml', resx: 'xml', xaml: 'xml', storyboard: 'xml', xib: 'xml', wxs: 'xml', pom: 'xml', iml: 'xml',
  sql: 'sql', mysql: 'mysql', pgsql: 'pgsql', psql: 'pgsql', graphql: 'graphql', gql: 'graphql', proto: 'protobuf',
  dockerfile: 'dockerfile', containerfile: 'dockerfile',
  makefile: 'makefile', mk: 'makefile', mak: 'makefile',
  cmake: 'cmake', tf: 'hcl', tfvars: 'hcl', hcl: 'hcl', nomad: 'hcl',
  zig: 'zig', nim: 'nim', ex: 'elixir', exs: 'elixir', erl: 'erlang', hrl: 'erlang', clj: 'clojure', cljs: 'clojure', cljc: 'clojure', edn: 'clojure', hs: 'haskell', lhs: 'haskell', ml: 'ocaml', mli: 'ocaml', elm: 'elm', sol: 'sol', move: 'rust', cairo: 'rust',
  tex: 'latex', latex: 'latex', bib: 'bibtex', sty: 'latex', cls: 'latex',
  txt: 'plaintext', log: 'log', lock: 'plaintext', csv: 'plaintext', tsv: 'plaintext', diff: 'diff', patch: 'diff',
  prisma: 'plaintext', wgsl: 'wgsl', glsl: 'c', frag: 'c', vert: 'c', hlsl: 'hlsl', metal: 'cpp',
  pug: 'pug', jade: 'pug', coffee: 'coffeescript', litcoffee: 'coffeescript', ts_: 'typescript',
  dockerignore: 'ini', gitignore: 'ini', gitattributes: 'ini', caretignore: 'ini', cursorignore: 'ini', mdc: 'markdown', mdc_: 'markdown',
  st: 'st', pas: 'pascal', pp: 'pascal', abap: 'abap', apex: 'apex', tcl: 'tcl', scm: 'scheme', rkt: 'scheme', lisp: 'scheme', el: 'scheme', sb: 'sb', m3: 'm3', redis: 'redis', ecl: 'ecl', bicep: 'bicep', azcli: 'azcli', rst: 'restructuredtext', pq: 'powerquery', msdax: 'msdax', cypher: 'cypher', qs: 'qsharp', vhd: 'vhdl', v: 'systemverilog', sv: 'systemverilog', svh: 'systemverilog', fsharp: 'fsharp', wasm: 'plaintext', wat: 'plaintext',
}

const FILENAMES: Record<string, string> = {
  dockerfile: 'dockerfile', containerfile: 'dockerfile', makefile: 'makefile', gnumakefile: 'makefile', 'cmakelists.txt': 'cmake',
  rakefile: 'ruby', gemfile: 'ruby', guardfile: 'ruby', podfile: 'ruby', fastfile: 'ruby', brewfile: 'ruby', vagrantfile: 'ruby', capfile: 'ruby',
  'package.json': 'json', 'tsconfig.json': 'json', 'composer.json': 'json', '.babelrc': 'json', '.eslintrc': 'json', '.prettierrc': 'json', '.swcrc': 'json', 'deno.json': 'json',
  '.env': 'ini', '.env.local': 'ini', '.env.example': 'ini', '.editorconfig': 'ini', '.gitconfig': 'ini', '.npmrc': 'ini', '.yarnrc': 'ini', '.gitignore': 'ini', '.gitattributes': 'ini', '.dockerignore': 'ini', '.caretignore': 'ini', '.cursorignore': 'ini', '.cursorindexingignore': 'ini', '.prettierignore': 'ini', '.eslintignore': 'ini',
  'cargo.lock': 'ini', 'cargo.toml': 'ini', 'pyproject.toml': 'ini', 'go.mod': 'plaintext', 'go.sum': 'plaintext', '.cursorrules': 'markdown', 'agents.md': 'markdown', 'claude.md': 'markdown', license: 'plaintext', readme: 'markdown', changelog: 'markdown',
  procfile: 'shell', '.bashrc': 'shell', '.zshrc': 'shell', '.bash_profile': 'shell', '.profile': 'shell', '.zprofile': 'shell', '.zshenv': 'shell',
  jenkinsfile: 'java', 'build.gradle': 'java', 'settings.gradle': 'java', 'build.gradle.kts': 'kotlin', 'settings.gradle.kts': 'kotlin', 'justfile': 'makefile', '.justfile': 'makefile', 'pipfile': 'ini', 'pipfile.lock': 'json',
}

export function languageForPath(path: string): string {
  const base = path.split(/[\\/]/).pop() ?? path
  const lower = base.toLowerCase()
  if (FILENAMES[lower]) return FILENAMES[lower]
  // dotfiles like ".zshrc" handled above; multi-dot like "foo.test.ts" → last ext; "a.d.ts" → ts
  const idx = lower.lastIndexOf('.')
  if (idx < 0) {
    // files without extension: heuristics
    if (lower.endsWith('file')) return 'plaintext'
    return 'plaintext'
  }
  const ext = lower.slice(idx + 1)
  if (ext === 'in' || ext === 'tpl' || ext === 'template' || ext === 'dist' || ext === 'bak' || ext === 'example' || ext === 'sample') {
    // "config.yaml.in" → use inner extension
    const inner = lower.slice(0, idx)
    const j = inner.lastIndexOf('.')
    if (j >= 0) { const e2 = inner.slice(j + 1); if (EXT[e2]) return EXT[e2] }
  }
  return EXT[ext] ?? 'plaintext'
}

/** Monaco language id → LSP `languageId` (per LSP spec identifiers). */
const LSP_IDS: Record<string, string> = {
  typescript: 'typescript', javascript: 'javascript', json: 'json', html: 'html', css: 'css', scss: 'scss', less: 'less',
  markdown: 'markdown', python: 'python', rust: 'rust', go: 'go', java: 'java', kotlin: 'kotlin', scala: 'scala', c: 'c', cpp: 'cpp', 'objective-c': 'objective-c',
  csharp: 'csharp', fsharp: 'fsharp', ruby: 'ruby', php: 'php', swift: 'swift', dart: 'dart', lua: 'lua', r: 'r', perl: 'perl', shell: 'shellscript', powershell: 'powershell',
  yaml: 'yaml', ini: 'ini', xml: 'xml', sql: 'sql', graphql: 'graphql', protobuf: 'proto', dockerfile: 'dockerfile', makefile: 'makefile', zig: 'zig', elixir: 'elixir', erlang: 'erlang', clojure: 'clojure', haskell: 'haskell', ocaml: 'ocaml', elm: 'elm', latex: 'latex', plaintext: 'plaintext', hcl: 'terraform', vb: 'vb', julia: 'julia',
}
export function lspLanguageId(monacoLanguage: string, path?: string): string {
  if (path) {
    const ext = path.split('.').pop()?.toLowerCase()
    if (ext === 'tsx') return 'typescriptreact'
    if (ext === 'jsx') return 'javascriptreact'
    if (ext === 'toml') return 'toml'
    if (ext === 'vue') return 'vue'
    if (ext === 'svelte') return 'svelte'
  }
  return LSP_IDS[monacoLanguage] ?? monacoLanguage
}

/** Human label for a language id (status bar). */
export function languageLabel(id: string): string {
  const map: Record<string, string> = { typescript: 'TypeScript', javascript: 'JavaScript', json: 'JSON', html: 'HTML', css: 'CSS', scss: 'SCSS', less: 'Less', markdown: 'Markdown', python: 'Python', rust: 'Rust', go: 'Go', java: 'Java', kotlin: 'Kotlin', c: 'C', cpp: 'C++', csharp: 'C#', ruby: 'Ruby', php: 'PHP', swift: 'Swift', shell: 'Shell Script', yaml: 'YAML', ini: 'INI/TOML', xml: 'XML', sql: 'SQL', plaintext: 'Plain Text', dockerfile: 'Dockerfile', makefile: 'Makefile', lua: 'Lua', dart: 'Dart', 'objective-c': 'Objective-C', graphql: 'GraphQL', hcl: 'HCL', zig: 'Zig', log: 'Log', diff: 'Diff' }
  return map[id] ?? id.charAt(0).toUpperCase() + id.slice(1)
}

export const IMAGE_EXTS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'ico', 'svg', 'avif', 'tiff', 'tif', 'heic'])
export function isImagePath(path: string): boolean { const e = path.split('.').pop()?.toLowerCase() ?? ''; return IMAGE_EXTS.has(e) }
export function isMarkdownPath(path: string): boolean { return languageForPath(path) === 'markdown' }

/** Comment token prefixes per language (for "suggest in comments" detection). */
export function lineCommentPrefix(language: string): string | null {
  switch (language) {
    case 'python': case 'shell': case 'yaml': case 'ruby': case 'perl': case 'r': case 'dockerfile': case 'makefile': case 'ini': case 'powershell': case 'elixir': case 'hcl': case 'julia': return '#'
    case 'lua': case 'sql': case 'haskell': case 'elm': case 'pgsql': case 'mysql': return '--'
    case 'html': case 'xml': case 'markdown': return null
    case 'clojure': case 'scheme': return ';'
    case 'latex': return '%'
    case 'vb': return "'"
    default: return '//'
  }
}
