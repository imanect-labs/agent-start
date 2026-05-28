import { memo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

/**
 * Renders assistant text as GitHub-flavored Markdown with themed, copyable
 * code blocks (U3). Syntax highlighting is intentionally omitted to keep
 * the bundle lean; blocks are clean monospace with a language label + copy.
 */
function CodeBlock({ className, children }: { className?: string; children: React.ReactNode }) {
  const [copied, setCopied] = useState(false);
  const lang = /language-(\w+)/.exec(className ?? "")?.[1] ?? "";
  const text = String(children).replace(/\n$/, "");

  const copy = async () => {
    const ok = await copyText(text);
    if (ok) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }
  };

  return (
    <div className="relative group my-2 rounded-lg border border-line bg-surface-sunken overflow-hidden">
      <div className="flex items-center justify-between px-3 py-1 border-b border-line/70 bg-surface-muted/60">
        <span className="text-[10px] uppercase tracking-wide text-fg-faint font-mono">
          {lang || "code"}
        </span>
        <button
          type="button"
          onClick={copy}
          className="text-[11px] px-1.5 py-0.5 rounded text-fg-subtle hover:text-fg hover:bg-surface-elev transition-colors"
        >
          {copied ? "コピー済" : "コピー"}
        </button>
      </div>
      <pre className="overflow-x-auto scroll-thin p-3 text-[12.5px] leading-relaxed font-mono text-fg">
        <code>{text}</code>
      </pre>
    </div>
  );
}

export const ChatMarkdown = memo(function ChatMarkdown({ text }: { text: string }) {
  return (
    <div className="chat-md text-[14px] leading-relaxed text-fg break-words">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code({ className, children, ...props }) {
            const isBlock =
              (className ?? "").includes("language-") || String(children).includes("\n");
            if (isBlock) {
              return <CodeBlock className={className}>{children}</CodeBlock>;
            }
            return (
              <code
                className="rounded bg-surface-muted px-1 py-0.5 text-[12.5px] font-mono text-fg"
                {...props}
              >
                {children}
              </code>
            );
          },
          a({ children, href }) {
            return (
              <a
                href={href}
                target="_blank"
                rel="noreferrer"
                className="text-accent underline underline-offset-2 hover:text-accent-hover"
              >
                {children}
              </a>
            );
          },
          ul({ children }) {
            return <ul className="list-disc pl-5 my-1.5 space-y-0.5">{children}</ul>;
          },
          ol({ children }) {
            return <ol className="list-decimal pl-5 my-1.5 space-y-0.5">{children}</ol>;
          },
          p({ children }) {
            return <p className="my-1.5 first:mt-0 last:mb-0">{children}</p>;
          },
          h1({ children }) {
            return <h1 className="text-base font-semibold mt-3 mb-1.5">{children}</h1>;
          },
          h2({ children }) {
            return <h2 className="text-[15px] font-semibold mt-3 mb-1.5">{children}</h2>;
          },
          h3({ children }) {
            return <h3 className="text-[14px] font-semibold mt-2.5 mb-1">{children}</h3>;
          },
          blockquote({ children }) {
            return (
              <blockquote className="border-l-2 border-line-strong pl-3 my-2 text-fg-muted">
                {children}
              </blockquote>
            );
          },
          table({ children }) {
            return (
              <div className="overflow-x-auto scroll-thin my-2">
                <table className="text-[13px] border-collapse">{children}</table>
              </div>
            );
          },
          th({ children }) {
            return (
              <th className="border border-line px-2 py-1 text-left font-medium">{children}</th>
            );
          },
          td({ children }) {
            return <td className="border border-line px-2 py-1">{children}</td>;
          },
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
});

async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // fall through
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}
