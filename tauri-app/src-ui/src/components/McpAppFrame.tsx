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
        className={`rounded border border-gray-300 dark:border-gray-600 bg-gray-50 dark:bg-gray-800 p-4 ${className}`}
        role="region"
        aria-label={title}
      >
        <p className="text-sm text-gray-500 dark:text-gray-400">Loading MCP App…</p>
      </div>
    );
  }

  if (error) {
    return (
      <div
        className={`rounded border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20 p-4 ${className}`}
        role="alert"
      >
        <p className="text-sm text-red-700 dark:text-red-300">{error}</p>
      </div>
    );
  }

  if (!html) {
    return null;
  }

  return (
    <div
      className={`rounded border border-gray-300 dark:border-gray-600 overflow-hidden bg-white dark:bg-gray-900 ${className}`}
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
