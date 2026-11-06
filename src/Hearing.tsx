import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  EVENTS,
  type HearingStatus,
  type Microphone,
  type ModelStatus,
  type TranscriptLine,
} from "./shared/chat";

/** Value of the "system default" option in the mic picker. */
const DEFAULT_MIC = "";
/** Lines kept in the transcript view. */
const MAX_TRANSCRIPT_LINES = 50;

function useHearingStatus() {
  const [status, setStatus] = useState<HearingStatus | null>(null);
  useEffect(() => {
    invoke<HearingStatus>("hearing_status").then(setStatus);
    const unlisten = listen<HearingStatus>(EVENTS.hearingStatus, (event) => setStatus(event.payload));
    return () => void unlisten.then((stop) => stop());
  }, []);
  return status;
}

function modelText(model: ModelStatus): string {
  switch (model.state) {
    case "checking":
      return "Checking the speech model…";
    case "downloading":
      return `Downloading the speech model (one time only)… ${model.percent}%`;
    case "loading":
      return "Loading the speech model…";
    case "ready":
      return "Speech recognition ready";
    case "failed":
      return model.error;
  }
}

/** Mic picker, level meter and speech model status. */
export function MicrophoneSettings() {
  const status = useHearingStatus();
  const [microphones, setMicrophones] = useState<Microphone[]>([]);
  const [level, setLevel] = useState(0);
  const [saveError, setSaveError] = useState<string | null>(null);

  const refreshMicrophones = () => invoke<Microphone[]>("list_microphones").then(setMicrophones);

  useEffect(() => {
    refreshMicrophones();
    const unlisten = listen<number>(EVENTS.micLevel, (event) => setLevel(event.payload));
    return () => void unlisten.then((stop) => stop());
  }, []);

  if (!status) return null;

  const chosen = status.microphone ?? DEFAULT_MIC;
  const chosenIsMissing =
    chosen !== DEFAULT_MIC && !microphones.some((microphone) => microphone.id === chosen);

  const choose = (id: string) => {
    setSaveError(null);
    invoke("set_microphone", { id: id === DEFAULT_MIC ? null : id }).catch((error) =>
      setSaveError(String(error)),
    );
  };

  // Loudness is small for normal speech; a square root spreads it out.
  const meterPercent = Math.min(100, Math.sqrt(level) * 180);

  return (
    <>
      <h2>Microphone</h2>
      <select
        className="mic-picker"
        value={chosen}
        onFocus={refreshMicrophones}
        onChange={(event) => choose(event.target.value)}
      >
        <option value={DEFAULT_MIC}>System default</option>
        {microphones.map((microphone) => (
          <option key={microphone.id} value={microphone.id}>
            {microphone.name}
          </option>
        ))}
        {chosenIsMissing && <option value={chosen}>Unplugged microphone (using default)</option>}
      </select>
      {saveError && <p className="error">{saveError}</p>}

      <div className="mic-meter" aria-hidden>
        <div style={{ width: `${status.listening ? meterPercent : 0}%` }} />
      </div>
      <p className="hint">
        {status.listening
          ? `Listening to ${status.microphoneName}`
          : "Listens only while a Session is running."}
      </p>

      <p className={`model-status model-${status.model.state}`}>
        {modelText(status.model)}
        {status.model.state === "failed" && (
          <button onClick={() => invoke("retry_speech_model")}>Try again</button>
        )}
      </p>
    </>
  );
}

/** What speech recognition heard, for checking that it works. */
export function Transcript() {
  const [lines, setLines] = useState<TranscriptLine[]>([]);
  const end = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unlisten = listen<TranscriptLine>(EVENTS.transcript, (event) =>
      setLines((previous) => [...previous, event.payload].slice(-MAX_TRANSCRIPT_LINES)),
    );
    return () => void unlisten.then((stop) => stop());
  }, []);

  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [lines]);

  return (
    <details className="transcript" open>
      <summary>What the chat hears</summary>
      <div className="transcript-lines">
        {lines.length === 0 ? (
          <p className="hint">Start a Session and say something. Nothing here is saved.</p>
        ) : (
          lines.map((line) => <p key={line.id}>{line.text}</p>)
        )}
        <div ref={end} />
      </div>
    </details>
  );
}
