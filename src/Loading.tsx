type SpinnerProps = { size?: "sm" | "lg" };

export function Spinner({ size = "sm" }: SpinnerProps) {
  return <span className={`spinner${size === "lg" ? " lg" : ""}`} aria-hidden />;
}

export default function PageLoading({ text = "加载中…" }: { text?: string }) {
  return (
    <div className="page-loading" role="status" aria-live="polite">
      <Spinner size="lg" />
      <p>{text}</p>
    </div>
  );
}
