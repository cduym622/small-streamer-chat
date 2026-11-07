import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface AiStatus {
  connected: boolean;
  /** The end of the saved key, e.g. "…a1b2". */
  keyHint: string | null;
}

/** Anthropic API key entry. The key is checked, then kept in the OS keychain. */
export function AiConnection() {
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [key, setKey] = useState("");
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<AiStatus>("ai_status")
      .then(setStatus)
      .catch((e) => setError(String(e)));
  }, []);

  const save = async () => {
    setChecking(true);
    setError(null);
    try {
      setStatus(await invoke<AiStatus>("save_anthropic_key", { key }));
      setKey("");
    } catch (e) {
      setError(String(e));
    } finally {
      setChecking(false);
    }
  };

  const remove = async () => {
    setError(null);
    try {
      await invoke("remove_anthropic_key");
      setStatus({ connected: false, keyHint: null });
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <>
      <h2>AI Connection</h2>
      {status?.connected ? (
        <p className="ai-connected">
          Claude Haiku, key {status.keyHint}
          <button onClick={remove}>Remove</button>
        </p>
      ) : (
        <>
          <p className="hint">
            Paste an Anthropic API key (create one at <code>console.anthropic.com</code> → API keys).
            Until then the chat uses canned lines.
          </p>
          <form
            className="key-form"
            onSubmit={(event) => {
              event.preventDefault();
              save();
            }}
          >
            <input
              type="password"
              placeholder="sk-ant-…"
              value={key}
              onChange={(event) => setKey(event.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
            <button className="primary" disabled={checking || key.trim() === ""}>
              {checking ? "Checking…" : "Test & save"}
            </button>
          </form>
        </>
      )}
      {error && <p className="error">{error}</p>}
    </>
  );
}
