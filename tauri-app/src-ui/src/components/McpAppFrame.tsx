/**
 * Renders an MCP App (ui:// resource) in a sandboxed iframe.
 * Rendering remains disabled until get_mcp_app_content is wired in native runtime.
 */

export interface McpAppFrameProps {
  serverId: string;
  resourceUri: string;
  title?: string;
  className?: string;
}

export default function McpAppFrame({
  serverId: _serverId,
  resourceUri: _resourceUri,
  title = "MCP App",
  className = "",
}: McpAppFrameProps) {
  return (
    <div
      className={`rounded border border-theme-border-dim bg-theme-bg-elevated p-4 ${className}`}
      role="region"
      aria-label={title}
    >
      <p className="text-sm text-theme-text-dim">
        MCP app rendering is disabled in this build.
      </p>
    </div>
  );
}
