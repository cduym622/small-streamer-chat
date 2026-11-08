/** Things that went wrong, each with a Dismiss button. */
export function Problems({
  problems,
  onDismiss,
}: {
  problems: string[];
  onDismiss: (problem: string) => void;
}) {
  if (problems.length === 0) return null;
  return (
    <div className="problems" role="alert">
      {problems.map((problem) => (
        <p key={problem}>
          {problem}
          <button onClick={() => onDismiss(problem)}>Dismiss</button>
        </p>
      ))}
    </div>
  );
}
