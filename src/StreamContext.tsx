import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** "What's the stream about?", saved when the field loses focus or on Enter. */
export function StreamContext({ onError }: { onError: (error: string) => void }) {
  const [context, setContext] = useState("");

  useEffect(() => {
    invoke<string>("stream_context").then(setContext);
  }, []);

  const save = () =>
    invoke("set_stream_context", { context }).catch((error) => onError(String(error)));

  return (
    <label className="stream-context">
      <span>What's the stream about?</span>
      <input
        placeholder="e.g. first time playing Elden Ring, chill vibes"
        value={context}
        maxLength={300}
        onChange={(event) => setContext(event.target.value)}
        onBlur={save}
        onKeyDown={(event) => event.key === "Enter" && event.currentTarget.blur()}
      />
    </label>
  );
}
