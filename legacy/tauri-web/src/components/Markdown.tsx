import { memo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Check, Copy } from "lucide-react";

function CodeBlock({ children, className }: { children?: any; className?: string }) {
  const [copied, setCopied] = useState(false);
  const lang = /language-(\w+)/.exec(className ?? "")?.[1];
  const text = () => {
    const walk = (n: any): string =>
      typeof n === "string" ? n : Array.isArray(n) ? n.map(walk).join("") : n?.props?.children ? walk(n.props.children) : "";
    return walk(children);
  };
  return (
    <div className="codeblock">
      <div className="codeblock-head">
        <span>{lang ?? "code"}</span>
        <button
          onClick={() => {
            navigator.clipboard.writeText(text());
            setCopied(true);
            setTimeout(() => setCopied(false), 1400);
          }}
        >
          {copied ? <Check size={13} /> : <Copy size={13} />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre>
        <code className={className}>{children}</code>
      </pre>
    </div>
  );
}

export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[[rehypeHighlight, { detect: true, ignoreMissing: true }]]}
        components={{
          pre: ({ children }) => <>{children}</>,
          code: ({ className, children, ...rest }) => {
            const block = /language-/.test(className ?? "") || String(children).includes("\n");
            return block ? (
              <CodeBlock className={className}>{children}</CodeBlock>
            ) : (
              <code className="inline" {...rest}>{children}</code>
            );
          },
          a: ({ href, children }) => (
            <a href={href} target="_blank" rel="noreferrer">{children}</a>
          ),
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
});
