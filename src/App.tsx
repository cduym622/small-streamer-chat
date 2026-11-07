import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./shared/chat.css";
import "./App.css";
import {
  CHATGPT_USAGE_URL,
  EVENTS,
  type ChatMessage,
  type EngineState,
} from "./shared/chat";
import { MicrophoneSettings, Transcript } from "./Hearing";
import { AiConnection } from "./AiConnection";

interface AppInfo {
  overlayUrl: string;
  hotkeys: { keys: string; action: string }[];
  problems: string[];
}

/** Lines kept in the app's Practice Chat view. */
const MAX_LINES = 200;

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [state, setState] = useState<EngineState>({
    running: false,
    backedOff: false,
    sessionCost: 0,
    provider: "none",
  });
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [problems, setProblems] = useState<string[]>([]);
  const [copied, setCopied] = useState(false);
  const [streamContext, setStreamContext] = useState("");
  const chatEnd = useRef<HTMLDivElement>(null);

  useEffect(() => {
    invoke<AppInfo>("app_info").then((appInfo) => {
      setInfo(appInfo);
      setProblems(appInfo.problems);
    });
    invoke<EngineState>("engine_state").then(setState);
    invoke<string>("stream_context").then(setStreamContext);

    const unlisten = [
      listen<EngineState>(EVENTS.state, (event) => setState(event.payload)),
      listen<ChatMessage>(EVENTS.message, (event) =>
        setMessages((previous) => [...previous, event.payload].slice(-MAX_LINES)),
      ),
      listen<string>(EVENTS.problem, (event) =>
        setProblems((previous) => [...previous, event.payload]),
      ),
    ];
    return () => unlisten.forEach((promise) => promise.then((stop) => stop()));
  }, []);

  useEffect(() => {
    chatEnd.current?.scrollIntoView({ block: "end" });
  }, [messages]);

  const copyOverlayUrl = async () => {
    if (!info) return;
    await navigator.clipboard.writeText(info.overlayUrl);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const saveStreamContext = () =>
    invoke("set_stream_context", { context: streamContext }).catch((error) =>
      setProblems((previous) => [...previous, String(error)]),
    );

  const dismissProblem = (problem: string) => {
    invoke("dismiss_problem", { message: problem });
    setProblems((previous) => previous.filter((p) => p !== problem));
  };

  const status = !state.running ? "Stopped" : state.backedOff ? "Backed off" : "Live";

  return (
    <div className="app">
      <header className="app-header">
        <h1>Small Streamer Chat</h1>
        <span className={`status status-${status.toLowerCase().replace(" ", "-")}`}>{status}</span>
        {state.provider === "chatGpt" ? (
          <span className="cost">
            Using ChatGPT plan ·{" "}
            <button className="link" onClick={() => openUrl(CHATGPT_USAGE_URL)}>
              Manage usage
            </button>
          </span>
        ) : (
          state.provider === "anthropic" && (
            <span
              className="cost"
              title="What this Session has cost on your Anthropic account so far"
            >
              Session cost ${state.sessionCost.toFixed(4)}
            </span>
          )
        )}
      </header>

      {problems.length > 0 && (
        <div className="problems" role="alert">
          {problems.map((problem) => (
            <p key={problem}>
              {problem}
              <button onClick={() => dismissProblem(problem)}>Dismiss</button>
            </p>
          ))}
        </div>
      )}

      <label className="stream-context">
        <span>What's the stream about?</span>
        <input
          placeholder="e.g. first time playing Elden Ring, chill vibes"
          value={streamContext}
          maxLength={300}
          onChange={(event) => setStreamContext(event.target.value)}
          onBlur={saveStreamContext}
          onKeyDown={(event) => event.key === "Enter" && event.currentTarget.blur()}
        />
      </label>

      <div className="controls">
        <button
          className="primary"
          onClick={() => invoke("set_running", { running: !state.running })}
        >
          {state.running ? "Stop" : "Start"}
        </button>
        <button
          disabled={!state.running}
          aria-pressed={state.backedOff}
          onClick={() => invoke("set_back_off", { backedOff: !state.backedOff })}
        >
          {state.backedOff ? "Resume" : "Back-off"}
        </button>
        <button disabled={!state.running} onClick={() => invoke("nudge")}>
          Nudge
        </button>
      </div>

      <main className="layout">
        <section className="practice-chat" aria-label="Practice Chat">
          {messages.length === 0 ? (
            <p className="empty">
              {state.running ? "The first message is on its way…" : "Press Start to begin a Session."}
            </p>
          ) : (
            messages.map((message) => (
              <div className="chat-line" key={message.id}>
                <span className="ai-badge">AI</span>
                <span className="chatter" style={{ color: message.color }}>
                  {message.chatter}
                </span>
                <span className="text">: {message.text}</span>
              </div>
            ))
          )}
          <div ref={chatEnd} />
        </section>

        <aside className="side">
          <AiConnection />

          <MicrophoneSettings />

          <h2>OBS Overlay</h2>
          <p>
            In OBS, add a <strong>Browser</strong> source with this URL. Its background is transparent.
          </p>
          <div className="overlay-url">
            <code>{info?.overlayUrl ?? "…"}</code>
            <button onClick={copyOverlayUrl}>{copied ? "Copied" : "Copy"}</button>
          </div>

          <h2>Hotkeys</h2>
          <p>These work while you're in a game.</p>
          <dl className="hotkeys">
            {info?.hotkeys.map((hotkey) => (
              <div key={hotkey.keys}>
                <dt>
                  <kbd>{hotkey.keys}</kbd>
                </dt>
                <dd>{hotkey.action}</dd>
              </div>
            ))}
          </dl>

          <Transcript />
        </aside>
      </main>
    </div>
  );
}

export default App;
