import type { FieldChangeDto } from "../../generated/query/FieldChangeDto";

interface FieldDiffProps {
  readonly field: FieldChangeDto;
}

/**
 * Renders one persisted field value. Scalars render as direct text; objects
 * and arrays render as pretty-printed JSON in a scrollable block so nothing
 * is truncated. Values that cannot be serialized (circular, bigint) fall back
 * to a placeholder instead of crashing the page.
 */
function DiffValue({ value }: { readonly value: unknown }) {
  if (value === null) {
    return <span className="timeline-diff-scalar timeline-diff-scalar--null">null</span>;
  }
  if (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return <span className="timeline-diff-scalar">{String(value)}</span>;
  }
  let rendered: string;
  try {
    const serialized = JSON.stringify(value, null, 2);
    rendered = serialized === undefined ? String(value) : serialized;
  } catch {
    rendered = "（无法序列化）";
  }
  return <pre className="timeline-diff-json">{rendered}</pre>;
}

export function FieldDiff({ field }: FieldDiffProps) {
  return (
    <details className="timeline-diff">
      <summary className="timeline-diff-summary">{field.path}</summary>
      <div className="timeline-diff-grid">
        <span className="timeline-diff-label timeline-diff-label--before">变更前</span>
        <div className="timeline-diff-value">
          <DiffValue value={field.before} />
        </div>
        <span className="timeline-diff-label timeline-diff-label--after">变更后</span>
        <div className="timeline-diff-value">
          <DiffValue value={field.after} />
        </div>
      </div>
    </details>
  );
}
