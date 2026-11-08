import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AiConnection } from "./AiConnection";
import { MicrophoneSettings, Transcript } from "./Hearing";
import { ObsOverlay } from "./ObsOverlay";
import { StreamContext } from "./StreamContext";

const STEPS = ["Welcome", "Microphone", "AI Connection", "Your stream", "OBS"] as const;

/**
 * The first-run setup: mic → speech model → AI Connection → Stream Context →
 * OBS. Shown on first launch and from "Setup guide"; every step can be
 * changed later in the main window.
 */
export function Setup({ overlayUrl, onDone }: { overlayUrl: string; onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const last = step === STEPS.length - 1;

  const finish = () => {
    invoke("finish_setup")
      .then(onDone)
      .catch((e) => setError(String(e)));
  };

  return (
    <div className="setup-backdrop">
      <div className="setup" role="dialog" aria-label="Setup">
        <ol className="setup-steps">
          {STEPS.map((name, index) => (
            <li key={name} className={index === step ? "current" : index < step ? "done" : ""}>
              {name}
            </li>
          ))}
        </ol>

        <div className="setup-body">
          {step === 0 && <Welcome />}
          {step === 1 && <MicrophoneStep />}
          {step === 2 && (
            <>
              <AiConnection />
              <p className="hint">
                You can skip this for now. Until you connect one, the chat posts canned lines
                instead of reacting to what you say.
              </p>
            </>
          )}
          {step === 3 && (
            <>
              <h2>What's your stream about?</h2>
              <p>
                A few words help the chat say relevant things: the game, what you're trying to do,
                the vibe. You can change it before each stream.
              </p>
              <StreamContext onError={setError} />
            </>
          )}
          {step === 4 && (
            <>
              <h2>Show the chat on stream</h2>
              <ObsOverlay url={overlayUrl} detailed />
              <p>
                Then press <strong>Start</strong> and talk. The chat answers after you pause.
              </p>
            </>
          )}
          {error && <p className="error">{error}</p>}
        </div>

        <div className="setup-buttons">
          {!last && (
            <button className="link" onClick={finish}>
              Skip setup
            </button>
          )}
          <span className="spacer" />
          {step > 0 && <button onClick={() => setStep(step - 1)}>Back</button>}
          <button className="primary" onClick={last ? finish : () => setStep(step + 1)}>
            {last ? "Finish" : "Next"}
          </button>
        </div>
      </div>
    </div>
  );
}

function Welcome() {
  return (
    <>
      <h2>Welcome to Small Streamer Chat</h2>
      <p>
        Streaming to an empty chat is hard. Small Streamer Chat listens to you and posts AI chat
        messages that react to what you say, so you get used to talking on stream.
      </p>
      <ul>
        <li>The messages appear here and, if you like, on your stream through OBS.</li>
        <li>
          It never posts to Twitch, YouTube or anywhere else, and never reads your real chat.
          Every message has an <span className="ai-badge">AI</span> badge.
        </li>
        <li>What you say is turned into text on this computer and is never saved.</li>
      </ul>
      <p>Setup takes about two minutes.</p>
    </>
  );
}

/** Opens the mic while shown, so the meter moves and speech shows up as text. */
function MicrophoneStep() {
  useEffect(() => {
    invoke("test_microphone", { testing: true });
    return () => void invoke("test_microphone", { testing: false });
  }, []);

  return (
    <>
      <MicrophoneSettings />
      <p>
        Say something. The bar should move, and once speech recognition is ready, your words
        appear below. {navigator.userAgent.includes("Mac") && "If your Mac asks, allow microphone access."}
      </p>
      <Transcript emptyText="Your words show up here. Nothing is saved." />
    </>
  );
}
