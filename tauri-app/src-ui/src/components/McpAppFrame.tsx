/**
 * Renders an MCP App (ui:// resource) in a sandboxed iframe.
 * Fetches HTML from the MCP server via get_mcp_app_content and displays it
 * with restricted sandbox for security. Consent/confirmation should be handled
 * by the parent before rendering tools that require_confirmation.
 */

import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect } from "react";

export interface McpAppFrameProps {
  serverId: string;
  resourceUri: string;
  title?: string;
  className?: string;
}

export default function McpAppFrame({
  serverId,
  resourceUri,
  title = "MCP App",
  className = "",
}: McpAppFrameProps) {
  const [html, setHtml] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<string>("get_mcp_app_content", { serverId, resourceUri })
      .then((content) => {
        if (!cancelled) {
          setHtml(content);
        }
      })
      .catch((e: string) => {
        if (!cancelled) {
          setError(String(e));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [serverId, resourceUri]);

  if (loading) {
    return (
      <div
        className={`rounded border border-theme-border-dim bg-theme-bg-elevated p-4 ${className}`}
        role="region"
        aria-label={title}
      >
        <p className="text-sm text-theme-text-dim">Loading MCP App…</p>
      </div>
    );
  }

  if (error) {
    return (
      <div
        className={`rounded border border-red-800 bg-theme-danger-dim p-4 ${className}`}
        role="alert"
      >
        <p className="text-sm text-theme-danger">{error}</p>
      </div>
    );
  }

  if (!html) {
    return null;
  }

  return (
    <div
      className={`rounded border border-theme-border-dim overflow-hidden bg-theme-bg-inset ${className}`}
      role="region"
      aria-label={title}
    >
      <iframe
        title={title}
        sandbox="allow-scripts allow-same-origin"
        srcDoc={html}
        className="w-full min-h-[200px] border-0"
        referrerPolicy="no-referrer"
      />
    </div>
  );
}
