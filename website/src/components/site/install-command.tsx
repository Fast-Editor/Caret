import { CopyButton } from "./copy-button";
import { cn } from "@/lib/utils";

export function InstallCommand({ command, className }: { command: string; className?: string }) {
  return (
    <div className={cn("flex items-center gap-2 rounded-lg border border-border bg-muted/40 pl-4 pr-1 py-1 font-mono text-[13px]", className)}>
      <span className="select-none text-muted-foreground">$</span>
      <code className="min-w-0 flex-1 overflow-x-auto whitespace-nowrap py-2">{command}</code>
      <CopyButton text={command} />
    </div>
  );
}
