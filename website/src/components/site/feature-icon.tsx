import { AtSign, BookOpen, Bot, Bug, Code2, GitBranch, MousePointer2, Plug, Route, Search, ShieldCheck, Sparkles, Terminal, Users, Wand2, type LucideIcon } from "lucide-react";

const ICONS: Record<string, LucideIcon> = {
  sparkles: Sparkles, wand: Wand2, bot: Bot, at: AtSign, book: BookOpen, search: Search, plug: Plug, users: Users,
  route: Route, shield: ShieldCheck, bug: Bug, terminal: Terminal, code: Code2, git: GitBranch, cursor: MousePointer2,
};

export function FeatureIcon({ name, className }: { name: string; className?: string }) {
  const Icon = ICONS[name] ?? Sparkles;
  return <Icon className={className} aria-hidden />;
}
