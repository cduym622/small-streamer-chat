import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CHATGPT_USAGE_URL, EVENTS, type Provider } from "./shared/chat";

interface AiStatus {
  /** The end of the saved Anthropic key, e.g. "…a1b2". */
  anthropicKeyHint: string | null;
  chatgpt: {
    email: string;
    /** False after the sign-in ended; signing in again keeps the account. */
    signedIn: boolean;
    model: string | null;
  } | null;
  active: Provider;
}

interface ChatGptModels {
  models: { slug: string; name: string }[];
  selected: string | null;
}

/**
 * How the Practice Chat's messages get written: the streamer's ChatGPT plan,
 * or an Anthropic API key (kept in the OS keychain).
 */
export function AiConnection() {
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    invoke<AiStatus>("ai_status")
      .then(setStatus)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen(EVENTS.aiStatus, refresh);
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [refresh]);

  return (
    <>
      <h2>AI Connection</h2>
      <ChatGptConnection status={status} onError={setError} />
      <AnthropicKey status={status} onError={setError} />
      {error && (
        <p className="error">
          {error} <button className="link" onClick={() => setError(null)}>Dismiss</button>
        </p>
      )}
    </>
  );
}

interface SectionProps {
  status: AiStatus | null;
  onError: (error: string | null) => void;
}

function ChatGptConnection({ status, onError }: SectionProps) {
  const [signingIn, setSigningIn] = useState(false);
  const [welcome, setWelcome] = useState(false);
  const account = status?.chatgpt;

  const signIn = async () => {
    setSigningIn(true);
    onError(null);
    try {
      const result = await invoke<{ firstTime: boolean }>("sign_in_with_chatgpt");
      setWelcome(result.firstTime);
    } catch (e) {
      if (String(e) !== "Sign-in cancelled.") onError(String(e));
    } finally {
      setSigningIn(false);
    }
  };

  const signOut = async () => {
    onError(null);
    try {
      const warning = await invoke<string | null>("sign_out_of_chatgpt");
      if (warning) onError(warning);
    } catch (e) {
      onError(String(e));
    }
  };

  if (account?.signedIn) {
    return (
      <>
        {welcome && (
          <div className="welcome" role="status">
            <strong>You're using your ChatGPT plan</strong>
            <p>
              The Practice Chat's messages now count toward your ChatGPT plan's usage, not a bill
              from this app (it's free). You can see and limit this app's usage in ChatGPT
              settings.
            </p>
            <button onClick={() => setWelcome(false)}>Got it</button>
          </div>
        )}
        <p className="ai-connected">
          <span>ChatGPT plan, {account.email || "signed in"}</span>
          <button onClick={signOut}>Sign out</button>
        </p>
        <ChatGptModelPicker model={account.model} onError={onError} />
        <p className="hint">
          Using ChatGPT plan ·{" "}
          <button className="link" onClick={() => openUrl(CHATGPT_USAGE_URL)}>
            Manage usage
          </button>
        </p>
      </>
    );
  }

  return (
    <>
      {account && (
        <p className="hint">
          Your ChatGPT sign-in for {account.email} has ended. Continue with ChatGPT to sign in
          again.
        </p>
      )}
      <div className="chatgpt-sign-in">
        <button className="chatgpt" disabled={signingIn} onClick={signIn}>
          {signingIn ? "Waiting for your browser…" : "Continue with ChatGPT"}
        </button>
        {signingIn && <button onClick={() => invoke("cancel_chatgpt_sign_in")}>Cancel</button>}
      </div>
      <p className="hint">Uses your ChatGPT Go, Plus or Pro plan. No API key needed.</p>
    </>
  );
}

function ChatGptModelPicker({
  model,
  onError,
}: {
  model: string | null;
  onError: (error: string | null) => void;
}) {
  const [models, setModels] = useState<ChatGptModels | null>(null);

  useEffect(() => {
    invoke<ChatGptModels>("chatgpt_models")
      .then(setModels)
      .catch((e) => onError(String(e)));
  }, [onError]);

  if (!models) return <p className="hint">Loading models…</p>;
  const selected = model ?? models.selected ?? "";

  return (
    <label className="model-picker">
      <span>Model</span>
      <select
        value={selected}
        onChange={(event) => {
          const slug = event.target.value;
          setModels({ ...models, selected: slug });
          invoke("set_chatgpt_model", { model: slug }).catch((e) => onError(String(e)));
        }}
      >
        {models.models.map((option) => (
          <option key={option.slug} value={option.slug}>
            {option.name}
          </option>
        ))}
      </select>
    </label>
  );
}

function AnthropicKey({ status, onError }: SectionProps) {
  const [key, setKey] = useState("");
  const [checking, setChecking] = useState(false);
  const hint = status?.anthropicKeyHint;
  const chatgptActive = status?.active === "chatGpt";

  const save = async () => {
    setChecking(true);
    onError(null);
    try {
      await invoke("save_anthropic_key", { key });
      setKey("");
    } catch (e) {
      onError(String(e));
    } finally {
      setChecking(false);
    }
  };

  const remove = () => {
    onError(null);
    invoke("remove_anthropic_key").catch((e) => onError(String(e)));
  };

  if (hint) {
    return (
      <p className={chatgptActive ? "ai-connected inactive" : "ai-connected"}>
        <span>
          Claude Haiku, key {hint}
          {chatgptActive && " (not used while signed in with ChatGPT)"}
        </span>
        <button onClick={remove}>Remove</button>
      </p>
    );
  }

  // One AI Connection is enough.
  if (chatgptActive) return null;

  return (
    <>
      <p className="hint">
        Or paste an Anthropic API key (create one at <code>console.anthropic.com</code> → API keys).
        Until you connect one, the chat uses canned lines.
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
  );
}
