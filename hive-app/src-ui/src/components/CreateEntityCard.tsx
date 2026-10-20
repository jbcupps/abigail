interface CreateEntityCardProps { onOpen: () => void }

export default function CreateEntityCard({ onOpen }: CreateEntityCardProps) {
  return <div className="rounded-theme-lg border border-dashed border-theme-border bg-theme-surface-dim p-4">
    <div className="mb-2 text-xs uppercase tracking-wide text-theme-text-dim">Entity Genesis</div>
    <h3 className="font-medium text-theme-text-bright">Begin with a spark.</h3>
    <p className="my-2 text-sm text-theme-text-dim">Discover a new companion through conversation, voice, or a story you have written.</p>
    <button type="button" onClick={onOpen} className="min-h-11 w-full rounded-theme-md bg-theme-primary px-3 py-2 text-sm font-medium text-white">Create or resume an Entity</button>
  </div>;
}
